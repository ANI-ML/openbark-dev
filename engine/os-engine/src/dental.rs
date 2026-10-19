//! Tooth charts for dogs and cats (ADR-0021).
//!
//! - The shared chart data, `packages/dental/chart.json`: the dentitions in
//!   the modified Triadan system (AVDC), the drawing order of each chart row,
//!   five schematic tooth outlines and the finding table. The app reads the
//!   same file, so the screen and the PDF draw from the same numbers.
//! - The tooth reader: teeth said in a line, by number ("one oh eight"), by
//!   name ("upper right fourth premolar"), as a range ("101 through 103") or as
//!   a group ("the lower incisors"). Names mean the same tooth in both species,
//!   so the reader needs no species.
//! - The AVDC code table: the model writes words and fields; the printed words
//!   and code are derived here, never written by the model.
//! - The faithfulness checks for charted teeth. A tooth said nowhere is a hard
//!   fail; everything else is a soft flag for the vet to look at.

use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::faithfulness::{DisputedNumber, UnsupportedNumber};
use crate::numbers::{extract_numbers, normalize_spoken};
use crate::transcript::{Segment, Transcript};

const CHART_JSON: &str = include_str!("../../../packages/dental/chart.json");

/// Whose mouth the chart draws. Only dogs and cats have a chart in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dentition {
    Canine,
    Feline,
}

impl Dentition {
    pub fn key(self) -> &'static str {
        match self {
            Dentition::Canine => "canine",
            Dentition::Feline => "feline",
        }
    }

    /// From a species key (`template::species_key`): "canine" or "feline".
    pub fn from_species_key(key: &str) -> Option<Self> {
        match key {
            "canine" => Some(Dentition::Canine),
            "feline" => Some(Dentition::Feline),
            _ => None,
        }
    }

    /// "dog" or "cat", for plain messages.
    pub fn animal(self) -> &'static str {
        match self {
            Dentition::Canine => "dog",
            Dentition::Feline => "cat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Jaw {
    Upper,
    Lower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToothType {
    Incisor,
    Canine,
    Premolar,
    Molar,
}

/// The schematic outline a tooth is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Incisor,
    Canine,
    Premolar,
    Carnassial,
    Molar,
}

#[derive(Debug, Deserialize)]
pub struct Tooth {
    /// Triadan id: "108".
    pub id: String,
    pub jaw: Jaw,
    pub side: Side,
    pub deciduous: bool,
    #[serde(rename = "type")]
    pub kind: ToothType,
    /// 1-4 within its type ("fourth premolar" is 4; the canine is 1).
    pub ordinal: u8,
    pub shape: Shape,
    pub dentitions: Vec<Dentition>,
}

impl Tooth {
    /// Position within the quadrant: 8 for 108.
    pub fn position(&self) -> u8 {
        self.id[1..].parse().unwrap_or(0)
    }
}

/// The drawing order of each row, patient's right (viewer's left) first.
#[derive(Debug, Deserialize)]
pub struct Rows {
    pub upper: Vec<String>,
    pub lower: Vec<String>,
    pub upper_deciduous: Vec<String>,
    pub lower_deciduous: Vec<String>,
}

impl Rows {
    /// The rows from top to bottom of the chart, with their names.
    pub fn top_to_bottom(&self) -> [(&'static str, &[String]); 4] {
        [
            ("upper", &self.upper),
            ("upper_deciduous", &self.upper_deciduous),
            ("lower_deciduous", &self.lower_deciduous),
            ("lower", &self.lower),
        ]
    }
}

#[derive(Debug, Deserialize)]
pub struct DentitionRows {
    pub canine: Rows,
    pub feline: Rows,
}

/// Sizes in screen pixels at 1x; the PDF scales them.
#[derive(Debug, Deserialize)]
pub struct Geometry {
    /// Width of one tooth's slot.
    pub slot: f32,
    /// The outline box: [20, 40].
    pub tooth: [f32; 2],
    /// Height of the neck (crown above, roots below) in the outline box.
    pub neck: f32,
    /// Middle of the crown, where a fracture's zigzag is drawn.
    pub crown_mid: f32,
}

/// Outlines as SVG paths in a 20 x 40 box, crown at the top, using only
/// absolute M, L, C and Z.
#[derive(Debug, Deserialize)]
pub struct Shapes {
    pub incisor: String,
    pub canine: String,
    pub premolar: String,
    pub carnassial: String,
    pub molar: String,
}

impl Shapes {
    pub fn get(&self, shape: Shape) -> &str {
        match shape {
            Shape::Incisor => &self.incisor,
            Shape::Canine => &self.canine,
            Shape::Premolar => &self.premolar,
            Shape::Carnassial => &self.carnassial,
            Shape::Molar => &self.molar,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GradeRange {
    pub min: u8,
    pub max: u8,
    /// The finding means nothing without its number (resorption type).
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MmUse {
    None,
    Optional,
    Required,
}

/// The mark a finding puts on the diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Glyph {
    None,
    Circle,
    X,
    Zigzag,
    Notch,
    Neck,
    Dot,
}

#[derive(Debug, Deserialize)]
pub struct DetailDef {
    pub id: String,
    pub words: String,
    /// The AVDC code part ("CCF"), or the stage letter for resorption ("b").
    pub code: String,
    /// Only with this grade (resorption 4a-4c need stage 4).
    pub with_grade: Option<u8>,
    /// Every group needs one of its words in the tooth's clause.
    pub cues: Vec<Vec<String>>,
    /// Words that rule this type out ("uncomplicated" for a complicated
    /// fracture, "no sectioning" for a sectioned extraction).
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// The kind of tooth a finding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToothKind {
    Deciduous,
    Permanent,
}

#[derive(Debug, Deserialize)]
pub struct FindingDef {
    pub id: String,
    /// Only on this kind of tooth: a persistent deciduous tooth is a 500-800
    /// tooth, an unerupted tooth a permanent one.
    #[serde(default)]
    pub teeth: Option<ToothKind>,
    /// At most one per tooth: a second, different item of it on the same
    /// tooth is a conflict (two resorption stages, two extraction techniques),
    /// and its types exclude each other.
    #[serde(default)]
    pub one_per_tooth: bool,
    /// The editor's grouping: presence, fracture, periodontal, resorption,
    /// development, condition, treatment, other.
    pub group: String,
    /// The editor's label for the choice.
    pub label: String,
    /// Printed words with no grade or millimetres.
    pub words: String,
    /// Printed words with the grade: "Mobility {n}", "Tooth resorption stage {stage}".
    pub words_grade: Option<String>,
    /// Printed words with millimetres: "Probing depth {mm} mm".
    pub words_mm: Option<String>,
    /// Code with no grade or detail ("T/FX"); none when AVDC has no code.
    pub code: Option<String>,
    /// Code with a grade: "M{grade}", "TR{grade}{sub}".
    pub code_grade: Option<String>,
    /// Code with a detail: "T/FX/{code}".
    pub code_detail: Option<String>,
    /// The legend line naming the AVDC index, when the finding has one.
    pub legend: Option<String>,
    pub grade: Option<GradeRange>,
    pub mm: MmUse,
    pub details: Vec<DetailDef>,
    /// May be planned ("needs to come out") rather than done or found.
    pub plannable: bool,
    /// Printed from its as-said text (`other`).
    pub text_required: bool,
    pub glyph: Glyph,
    /// One of these words must be in the cited lines. Empty: no cue check.
    pub cues: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Chart {
    /// The finding table's version: it changes whenever a finding, a detail,
    /// a word or a code changes. Each saved run records it.
    pub version: u32,
    pub teeth: Vec<Tooth>,
    pub rows: DentitionRows,
    pub geometry: Geometry,
    pub shapes: Shapes,
    pub findings: Vec<FindingDef>,
}

impl Chart {
    pub fn tooth(&self, id: &str) -> Option<&Tooth> {
        self.teeth.iter().find(|t| t.id == id)
    }

    pub fn rows(&self, dentition: Dentition) -> &Rows {
        match dentition {
            Dentition::Canine => &self.rows.canine,
            Dentition::Feline => &self.rows.feline,
        }
    }

    pub fn finding(&self, id: &str) -> Option<&FindingDef> {
        self.findings.iter().find(|f| f.id == id)
    }
}

static CHART: LazyLock<Chart> =
    LazyLock::new(|| serde_json::from_str(CHART_JSON).expect("packages/dental/chart.json is valid"));

/// The shared chart data, loaded once.
pub fn chart() -> &'static Chart {
    &CHART
}

pub fn tooth(id: &str) -> Option<&'static Tooth> {
    chart().tooth(id)
}

/// The dentition's teeth (permanent and deciduous) in Triadan order. With no
/// dentition, the dog's: it contains every cat tooth.
pub fn ids(dentition: Option<Dentition>) -> Vec<&'static str> {
    let d = dentition.unwrap_or(Dentition::Canine);
    chart().teeth.iter().filter(|t| t.dentitions.contains(&d)).map(|t| t.id.as_str()).collect()
}

pub fn has_tooth(dentition: Option<Dentition>, id: &str) -> bool {
    let d = dentition.unwrap_or(Dentition::Canine);
    tooth(id).is_some_and(|t| t.dentitions.contains(&d))
}

/// Sort key for Triadan order: 101 … 411, then 501 … 808; unknown ids last.
pub fn triadan_order(id: &str) -> u32 {
    if tooth(id).is_some() { id.parse().unwrap_or(u32::MAX) } else { u32::MAX }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// One drawing command of an outline, in its 20 x 40 box (y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    Move(f32, f32),
    Line(f32, f32),
    Curve(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Parse an outline from `chart.json`: absolute `M`, `L`, `C` and `Z` only.
pub fn parse_path(d: &str) -> Result<Vec<PathCmd>, String> {
    fn numbers(parts: &mut std::str::SplitWhitespace<'_>, n: usize) -> Result<Vec<f32>, String> {
        (0..n)
            .map(|_| {
                parts
                    .next()
                    .ok_or_else(|| "a command is missing its numbers".to_string())?
                    .parse::<f32>()
                    .map_err(|e| e.to_string())
            })
            .collect()
    }
    let mut out = Vec::new();
    let mut parts = d.split_whitespace();
    while let Some(cmd) = parts.next() {
        match cmd {
            "M" => {
                let p = numbers(&mut parts, 2)?;
                out.push(PathCmd::Move(p[0], p[1]));
            }
            "L" => {
                let p = numbers(&mut parts, 2)?;
                out.push(PathCmd::Line(p[0], p[1]));
            }
            "C" => {
                let p = numbers(&mut parts, 6)?;
                out.push(PathCmd::Curve(p[0], p[1], p[2], p[3], p[4], p[5]));
            }
            "Z" => out.push(PathCmd::Close),
            other => return Err(format!("unsupported path command \"{other}\"")),
        }
    }
    if !matches!(out.first(), Some(PathCmd::Move(..))) {
        return Err("a path starts with M".into());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Items, the code table and the shape check
// ---------------------------------------------------------------------------

/// The fields of a chart item, read leniently from the note's JSON.
struct View<'a> {
    text: &'a str,
    finding: &'a str,
    detail: Option<&'a str>,
    grade: Option<f64>,
    mm: Option<f64>,
    planned: bool,
}

fn view(item: &Value) -> View<'_> {
    View {
        text: item["text"].as_str().unwrap_or_default(),
        finding: item["finding"].as_str().unwrap_or_default(),
        detail: item["detail"].as_str(),
        grade: item["grade"].as_f64(),
        mm: item["mm"].as_f64(),
        planned: item["planned"].as_bool().unwrap_or(false),
    }
}

/// Is this note item a charted tooth finding?
pub fn is_tooth_item(map: &Map<String, Value>) -> bool {
    map.get("tooth").is_some_and(Value::is_string) && map.get("finding").is_some_and(Value::is_string)
}

/// "7", "2.5": millimetres as said, without a trailing ".0".
fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e9 { format!("{}", n as i64) } else { format!("{n}") }
}

/// Fill `{name}` placeholders.
pub fn fill(text: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(text.to_string(), |acc, (k, v)| acc.replace(&format!("{{{k}}}"), v))
}

/// "Open extraction" → "open extraction", for words inside a sentence.
pub fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How one finding is printed: its words (a translation key with values),
/// its AVDC code when one exists, and whether it is a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Printed {
    /// English words with placeholders, which is also the translation key:
    /// "Mobility {n}". For `other` and for items whose fields don't fit, the
    /// as-said text.
    pub key: String,
    /// Values for the placeholders: n, stage, mm.
    pub vars: Vec<(&'static str, String)>,
    pub code: Option<String>,
    /// Printed as "Planned: {finding}".
    pub planned: bool,
}

impl Printed {
    /// The words and code through a translation function: "Planned: open
    /// extraction (XSS)". `lower_inner` lowercases the first letter of the
    /// words inside "Planned: …" (not in German, where nouns keep capitals).
    pub fn render(&self, tr: &dyn Fn(&str) -> String, lower_inner: bool) -> String {
        let mut words = fill(&tr(&self.key), &self.vars);
        if self.planned {
            let inner = if lower_inner { lower_first(&words) } else { words };
            words = fill(&tr("Planned: {finding}"), &[("finding", inner)]);
        }
        match &self.code {
            Some(code) => format!("{words} ({code})"),
            None => words,
        }
    }

    /// In English: "Periodontal disease stage 3 (PD3)".
    pub fn english(&self) -> String {
        self.render(&|k| k.to_string(), true)
    }
}

/// The printed words and AVDC code of an item (ADR-0021 decision 3).
pub fn code_of(item: &Value) -> Printed {
    let v = view(item);
    let as_said = || Printed { key: v.text.to_string(), vars: Vec::new(), code: None, planned: v.planned };
    let Some(f) = chart().finding(v.finding) else { return as_said() };
    if f.text_required || fits(item).is_err() {
        return as_said();
    }
    let detail = v.detail.and_then(|d| f.details.iter().find(|x| x.id == d));
    let mut p = Printed { key: f.words.clone(), vars: Vec::new(), code: f.code.clone(), planned: v.planned };
    if let (Some(mm), Some(words)) = (v.mm, &f.words_mm) {
        p.key = words.clone();
        p.vars.push(("mm", fmt_num(mm)));
    }
    if let (Some(g), Some(words)) = (v.grade, &f.words_grade) {
        let g = g as u8;
        let sub = detail.filter(|d| d.with_grade.is_some()).map(|d| d.code.as_str()).unwrap_or("");
        p.key = words.clone();
        p.vars.push(("n", g.to_string()));
        p.vars.push(("stage", format!("{g}{sub}")));
        if let Some(pattern) = &f.code_grade {
            p.code = Some(pattern.replace("{grade}", &g.to_string()).replace("{sub}", sub));
        }
    }
    if let Some(d) = detail.filter(|d| d.with_grade.is_none()) {
        p.key = d.words.clone();
        p.code = f.code_detail.as_ref().map(|pattern| pattern.replace("{code}", &d.code));
    }
    p
}

/// The mark an item puts on the diagram.
pub fn glyph_of(item: &Value) -> Glyph {
    chart().finding(view(item).finding).map_or(Glyph::Dot, |f| f.glyph)
}

/// Why an item's fields don't fit its finding, or a tooth doesn't fit the
/// mouth. The messages are plain sentences for the edit command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Misfit {
    UnknownFinding(String),
    UnknownDetail {
        finding: String,
        detail: String,
    },
    /// A resorption sub-stage (4a-4c) on a stage other than 4.
    DetailNeedsGrade {
        detail: String,
        finding: String,
        grade: u8,
    },
    GradeNotUsed {
        finding: String,
    },
    GradeMissing {
        finding: String,
        word: &'static str,
        min: u8,
        max: u8,
    },
    GradeRange {
        finding: String,
        word: &'static str,
        min: u8,
        max: u8,
    },
    MmNotUsed {
        finding: String,
    },
    MmMissing {
        finding: String,
    },
    MmRange {
        finding: String,
    },
    NotPlannable {
        finding: String,
    },
    TextMissing,
    UnknownTooth(String),
    NotInMouth {
        tooth: String,
        animal: &'static str,
    },
    /// A finding on the wrong kind of tooth: a persistent deciduous tooth on
    /// a permanent number.
    WrongKind {
        finding: String,
        tooth: String,
        kind: ToothKind,
    },
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Misfit::UnknownFinding(x) => write!(f, "\"{x}\" isn't a finding the chart knows."),
            Misfit::UnknownDetail { finding, detail } => write!(f, "{finding} can't be \"{detail}\"."),
            Misfit::DetailNeedsGrade { detail, finding, grade } => {
                write!(f, "\"{detail}\" is only for {} stage {grade}.", lower_first(finding))
            }
            Misfit::GradeNotUsed { finding } => write!(f, "{finding} takes no number."),
            Misfit::GradeMissing { finding, word, min, max } => {
                write!(f, "{finding} needs a {word} from {min} to {max}.")
            }
            Misfit::GradeRange { finding, word, min, max } => {
                write!(f, "{finding} takes a {word} from {min} to {max}.")
            }
            Misfit::MmNotUsed { finding } => write!(f, "{finding} takes no millimetres."),
            Misfit::MmMissing { finding } => write!(f, "{finding} needs a measurement in millimetres."),
            Misfit::MmRange { finding } => write!(f, "{finding} needs a measurement of 0 mm or more."),
            Misfit::NotPlannable { finding } => write!(f, "{finding} can't be planned."),
            Misfit::TextMissing => write!(f, "Other needs a description."),
            Misfit::UnknownTooth(t) => write!(f, "\"{t}\" isn't a tooth number."),
            Misfit::NotInMouth { tooth, animal } => write!(f, "{tooth} isn't a {animal}'s tooth."),
            Misfit::WrongKind { finding, tooth, kind: ToothKind::Deciduous } => {
                write!(f, "{finding} is only for deciduous teeth (500 to 800), not {tooth}.")
            }
            Misfit::WrongKind { finding, tooth, kind: ToothKind::Permanent } => {
                write!(f, "{finding} is only for permanent teeth (100 to 400), not {tooth}.")
            }
        }
    }
}

/// The word for a finding's number in messages.
fn grade_word(finding: &str) -> &'static str {
    match finding {
        "periodontal_stage" | "resorption" => "stage",
        "resorption_type" => "type",
        _ => "grade",
    }
}

/// Do the item's `detail`, `grade`, `mm` and `planned` fit its finding?
pub fn fits(item: &Value) -> Result<(), Misfit> {
    let v = view(item);
    let f = chart().finding(v.finding).ok_or_else(|| Misfit::UnknownFinding(v.finding.to_string()))?;
    let name = || f.words.clone();
    if f.text_required && v.text.trim().is_empty() {
        return Err(Misfit::TextMissing);
    }
    match (&f.grade, v.grade) {
        (None, Some(_)) => return Err(Misfit::GradeNotUsed { finding: name() }),
        (Some(r), None) if r.required => {
            return Err(Misfit::GradeMissing {
                finding: name(),
                word: grade_word(&f.id),
                min: r.min,
                max: r.max,
            });
        }
        (Some(r), Some(g)) if g.fract() != 0.0 || !(f64::from(r.min)..=f64::from(r.max)).contains(&g) => {
            return Err(Misfit::GradeRange {
                finding: name(),
                word: grade_word(&f.id),
                min: r.min,
                max: r.max,
            });
        }
        _ => {}
    }
    match (f.mm, v.mm) {
        (MmUse::None, Some(_)) => return Err(Misfit::MmNotUsed { finding: name() }),
        (MmUse::Required, None) => return Err(Misfit::MmMissing { finding: name() }),
        (_, Some(mm)) if !mm.is_finite() || mm < 0.0 => return Err(Misfit::MmRange { finding: name() }),
        _ => {}
    }
    if let Some(d) = v.detail {
        let def = f
            .details
            .iter()
            .find(|x| x.id == d)
            .ok_or_else(|| Misfit::UnknownDetail { finding: name(), detail: d.to_string() })?;
        if let Some(g) = def.with_grade
            && v.grade != Some(f64::from(g))
        {
            return Err(Misfit::DetailNeedsGrade { detail: def.words.clone(), finding: name(), grade: g });
        }
    }
    if v.planned && !f.plannable {
        return Err(Misfit::NotPlannable { finding: name() });
    }
    if let (Some(kind), Some(t)) = (f.teeth, item["tooth"].as_str().and_then(tooth)) {
        let fits_kind = match kind {
            ToothKind::Deciduous => t.deciduous,
            ToothKind::Permanent => !t.deciduous,
        };
        if !fits_kind {
            return Err(Misfit::WrongKind { finding: name(), tooth: t.id.clone(), kind });
        }
    }
    Ok(())
}

/// Is `id` a tooth of the dentition (any dog tooth when none is set)?
pub fn tooth_misfit(id: &str, dentition: Option<Dentition>) -> Option<Misfit> {
    let t = match tooth(id) {
        Some(t) => t,
        None => return Some(Misfit::UnknownTooth(id.to_string())),
    };
    match dentition {
        Some(d) if !t.dentitions.contains(&d) => {
            Some(Misfit::NotInMouth { tooth: id.to_string(), animal: d.animal() })
        }
        _ => None,
    }
}

/// The items grouped by tooth, in Triadan order; each tooth's findings keep
/// their order in the array.
pub fn by_tooth(items: &[Value]) -> Vec<(String, Vec<&Value>)> {
    let mut out: Vec<(String, Vec<&Value>)> = Vec::new();
    for item in items {
        let id = item["tooth"].as_str().unwrap_or_default().to_string();
        match out.iter_mut().find(|(t, _)| *t == id) {
            Some((_, list)) => list.push(item),
            None => out.push((id, vec![item])),
        }
    }
    out.sort_by(|a, b| triadan_order(&a.0).cmp(&triadan_order(&b.0)).then_with(|| a.0.cmp(&b.0)));
    out
}

/// A legend line: an AVDC index named in words, or one code with its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegendLine {
    /// "M0-M3: tooth mobility index (AVDC). Grades as said."
    Index(&'static str),
    /// "XSS" and the key of its words ("Open extraction").
    Code { code: String, words: String },
}

/// The legend for the codes these items use, in finding-table order. The
/// fixed lines ("Blank: not charted…", "Right and left…") are the renderer's.
pub fn legend(items: &[Value]) -> Vec<LegendLine> {
    let mut out: Vec<LegendLine> = Vec::new();
    for f in &chart().findings {
        for item in items.iter().filter(|i| view(i).finding == f.id) {
            let p = code_of(item);
            // The code's own words, without a measurement: "GR: gingival recession".
            let words = view(item)
                .detail
                .and_then(|d| f.details.iter().find(|x| x.id == d && x.with_grade.is_none()))
                .map_or(&f.words, |d| &d.words);
            let line = match (&f.legend, &p.code) {
                (Some(index), Some(_)) => LegendLine::Index(index.as_str()),
                (None, Some(code)) => LegendLine::Code { code: code.clone(), words: words.clone() },
                // "Probing depth is in millimetres." has no code to explain.
                (Some(index), None) if p.vars.iter().any(|(k, _)| *k == "mm") => {
                    LegendLine::Index(index.as_str())
                }
                _ => continue,
            };
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tooth names
// ---------------------------------------------------------------------------

/// A tooth's name in parts, for the app and the PDF to translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToothName {
    pub side: Side,
    pub jaw: Jaw,
    pub deciduous: bool,
    /// "fourth premolar", "canine".
    pub tooth: &'static str,
}

impl ToothName {
    /// "right maxillary fourth premolar", "left mandibular deciduous canine".
    pub fn english(&self) -> String {
        let side = match self.side {
            Side::Right => "right",
            Side::Left => "left",
        };
        let jaw = match self.jaw {
            Jaw::Upper => "maxillary",
            Jaw::Lower => "mandibular",
        };
        let deciduous = if self.deciduous { "deciduous " } else { "" };
        format!("{side} {jaw} {deciduous}{}", self.tooth)
    }
}

fn tooth_word(kind: ToothType, ordinal: u8) -> &'static str {
    match (kind, ordinal) {
        (ToothType::Incisor, 1) => "first incisor",
        (ToothType::Incisor, 2) => "second incisor",
        (ToothType::Incisor, _) => "third incisor",
        (ToothType::Canine, _) => "canine",
        (ToothType::Premolar, 1) => "first premolar",
        (ToothType::Premolar, 2) => "second premolar",
        (ToothType::Premolar, 3) => "third premolar",
        (ToothType::Premolar, _) => "fourth premolar",
        (ToothType::Molar, 1) => "first molar",
        (ToothType::Molar, 2) => "second molar",
        (ToothType::Molar, _) => "third molar",
    }
}

pub fn name_of(id: &str) -> Option<ToothName> {
    let t = tooth(id)?;
    Some(ToothName { side: t.side, jaw: t.jaw, deciduous: t.deciduous, tooth: tooth_word(t.kind, t.ordinal) })
}

// ---------------------------------------------------------------------------
// The tooth reader
// ---------------------------------------------------------------------------

/// How a tooth was said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SaidAs {
    Number,
    Name,
    Range,
    Group,
}

/// Teeth said in one place of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub teeth: Vec<String>,
    pub said_as: SaidAs,
    /// Word positions in the line, start inclusive, end exclusive.
    pub words: (usize, usize),
}

impl Mention {
    pub fn covers(&self, id: &str) -> bool {
        self.teeth.iter().any(|t| t == id)
    }

    /// Words between two mentions (0 when they touch or overlap).
    fn gap(&self, other: &Mention) -> usize {
        other.words.0.saturating_sub(self.words.1).max(self.words.0.saturating_sub(other.words.1))
    }
}

#[derive(Debug, Clone)]
struct Tok {
    w: String,
    /// Clause punctuation (, . ; : ! ? brackets, quotes) before this word.
    brk: bool,
    /// Sentence punctuation (. ; ! ?) before this word: a new sentence.
    stop: bool,
    /// A hyphen or en dash before this word.
    hyphen: bool,
    /// The word's characters in the text, start inclusive, end exclusive.
    span: (usize, usize),
}

/// Lowercase words with the punctuation between them. Decimals ("4.5") and
/// thousands ("1,250") stay one word; "hasn't" becomes "has not".
fn tokens(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().map(|c| if c == '\u{2019}' { '\'' } else { c }).collect();
    let mut out = Vec::new();
    let (mut brk, mut stop, mut hyphen) = (false, false, false);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !c.is_alphanumeric() {
            match c {
                '-' | '\u{2013}' => hyphen = true,
                '.' | ';' | '!' | '?' => {
                    brk = true;
                    stop = true;
                }
                // A colon introduces what follows ("Probing depths: 309 is
                // nine millimetres, 409 is three"): a clause, not a sentence.
                ':' | ',' | '(' | ')' | '[' | ']' | '"' | '\u{2014}' | '\u{201c}' | '\u{201d}' => brk = true,
                _ => {}
            }
            i += 1;
            continue;
        }
        let from = i;
        let mut word = String::new();
        while i < chars.len() {
            let c = chars[i];
            let numeric =
                !word.is_empty() && word.chars().all(|x| x.is_ascii_digit() || x == '.' || x == ',');
            let digit_at = |k: usize| chars.get(k).is_some_and(char::is_ascii_digit);
            let inside = c.is_alphanumeric()
                // "o'clock", "we'll"
                || (c == '\'' && !word.is_empty() && chars.get(i + 1).is_some_and(|n| n.is_alphabetic()))
                // "4.5"
                || (c == '.' && numeric && digit_at(i + 1))
                // "1,250"
                || (c == ',' && numeric && (1..=3).all(|k| digit_at(i + k)) && !digit_at(i + 4));
            if !inside {
                break;
            }
            word.push(c);
            i += 1;
        }
        let lower = word.to_lowercase();
        let span = (from, i);
        match lower.strip_suffix("n't") {
            Some(base) => {
                if !base.is_empty() {
                    out.push(Tok { w: base.to_string(), brk, stop, hyphen, span });
                }
                let first = base.is_empty();
                out.push(Tok {
                    w: "not".into(),
                    brk: first && brk,
                    stop: first && stop,
                    hyphen: false,
                    span,
                });
            }
            None => out.push(Tok { w: lower, brk, stop, hyphen, span }),
        }
        brk = false;
        stop = false;
        hyphen = false;
    }
    out
}

/// The text's characters, as [`tokens`] counts them.
fn chars_of(text: &str) -> Vec<char> {
    text.chars().map(|c| if c == '\u{2019}' { '\'' } else { c }).collect()
}

#[derive(Clone, Copy)]
enum Digit {
    One(u32),
    Zero,
    Teen(u32),
    /// "08" written as two digits.
    TwoZero(u32),
}

fn digit_class(w: &str) -> Option<Digit> {
    Some(match w {
        "one" | "1" => Digit::One(1),
        "two" | "2" => Digit::One(2),
        "three" | "3" => Digit::One(3),
        "four" | "4" => Digit::One(4),
        "five" | "5" => Digit::One(5),
        "six" | "6" => Digit::One(6),
        "seven" | "7" => Digit::One(7),
        "eight" | "8" => Digit::One(8),
        "nine" | "9" => Digit::One(9),
        "oh" | "o" | "zero" | "0" => Digit::Zero,
        "ten" | "10" => Digit::Teen(10),
        "eleven" | "11" => Digit::Teen(11),
        _ if w.len() == 2 && w.starts_with('0') && w.as_bytes()[1].is_ascii_digit() => {
            Digit::TwoZero(u32::from(w.as_bytes()[1] - b'0'))
        }
        _ => return None,
    })
}

/// Words after a number that make it a dose, a measure or a time, not a tooth.
#[rustfmt::skip]
const NOT_TOOTH_AFTER: &[&str] = &[
    "mg", "milligram", "milligrams", "mcg", "microgram", "micrograms", "g", "gram", "grams", "kg", "kilo",
    "kilos", "kilogram", "kilograms", "ml", "mls", "mil", "mils", "milliliter", "milliliters", "millilitre",
    "millilitres", "cc", "l", "litre", "liter", "litres", "liters", "unit", "units", "iu", "pm", "am",
    "oclock", "o'clock", "percent", "mm", "millimeter", "millimeters", "millimetre", "millimetres", "cm",
    "degree", "degrees", "bpm", "beats", "breaths", "minute", "minutes", "min", "mins", "hour", "hours", "hr",
    "hrs", "day", "days", "week", "weeks", "month", "months", "year", "years", "dollars", "mmhg", "second",
    "seconds", "sec", "times", "point", "pound", "pounds", "lb", "lbs", "tablet", "tablets", "tabs",
    "capsule", "capsules", "per",
];

/// Words shortly before a number that make it a vital sign or a price.
#[rustfmt::skip]
const NOT_TOOTH_BEFORE: &[&str] = &[
    "rate", "temperature", "temp", "weight", "weighs", "weighed", "pressure", "bp", "hr", "rr", "pulse",
    "spo2", "glucose", "pcv", "cost", "costs", "price", "total", "systolic", "diastolic", "saturation",
    "lactate",
];

/// The number said at `i` ("108", "one oh eight", "three ten", "1 08",
/// "one hundred and eight") and how many words it takes.
fn number_at(toks: &[Tok], i: usize) -> Option<(u32, usize)> {
    let w = toks[i].w.as_str();
    if w.len() == 3 && w.bytes().all(|b| b.is_ascii_digit()) {
        return Some((w.parse().ok()?, 1));
    }
    let Some(Digit::One(q)) = digit_class(w) else { return None };
    // The words of a spoken number sit in one clause.
    let joined = |k: usize| (1..k).all(|j| toks.get(i + j).is_some_and(|t| !t.brk));
    let class = |k: usize| toks.get(k).and_then(|t| digit_class(&t.w));
    if let (Some(Digit::Zero), Some(Digit::One(p))) = (class(i + 1), class(i + 2))
        && joined(3)
    {
        return Some((q * 100 + p, 3));
    }
    match class(i + 1) {
        Some(Digit::Teen(t)) if joined(2) => return Some((q * 100 + t, 2)),
        Some(Digit::TwoZero(p)) if joined(2) => return Some((q * 100 + p, 2)),
        _ => {}
    }
    if toks.get(i + 1).is_some_and(|t| t.w == "hundred") {
        let k = if toks.get(i + 2).is_some_and(|t| t.w == "and") { i + 3 } else { i + 2 };
        if let Some(Digit::One(p) | Digit::Teen(p)) = class(k)
            && joined(k - i + 1)
        {
            return Some((q * 100 + p, k - i + 1));
        }
    }
    None
}

/// Is the number at `i..i+len` a dose, measure, time or vital sign?
fn not_a_tooth(toks: &[Tok], i: usize, len: usize) -> bool {
    if let Some(next) = toks.get(i + len).filter(|t| !t.brk) {
        let w = next.w.as_str();
        let pm = matches!(w, "p" | "a") && toks.get(i + len + 1).is_some_and(|t| t.w == "m");
        if NOT_TOOTH_AFTER.contains(&w) || pm {
            return true;
        }
    }
    // "three ten" right after "at" is a time.
    let spoken_teen = len == 2 && digit_class(&toks[i + 1].w).is_some_and(|d| matches!(d, Digit::Teen(_)));
    let mut k = i;
    for _ in 0..3 {
        if k == 0 || toks[k].brk {
            break;
        }
        k -= 1;
        let w = toks[k].w.as_str();
        if NOT_TOOTH_BEFORE.contains(&w) || (spoken_teen && w == "at" && k + 1 == i) {
            return true;
        }
    }
    false
}

/// "101 through 203": a range along one arcade, across the midline too.
const RANGE_THROUGH: &[&str] = &["through", "thru"];
/// "106 to 108", "106-108": a range only inside one quadrant. Across it,
/// "to" is as often a movement ("from 108 to 208") as a range.
const RANGE_TO: &[&str] = &["to", "till", "until"];

/// The teeth from `a` to `b` along one arcade (dog rows, crossing the
/// midline), in the order said. None across jaws or between permanent and
/// deciduous rows.
fn arcade(a: &str, b: &str) -> Option<Vec<String>> {
    if a == b {
        return None;
    }
    let rows = chart().rows(Dentition::Canine);
    for (_, row) in rows.top_to_bottom() {
        let (Some(ia), Some(ib)) = (row.iter().position(|t| t == a), row.iter().position(|t| t == b)) else {
            continue;
        };
        let slice: Vec<String> =
            if ia <= ib { row[ia..=ib].to_vec() } else { row[ib..=ia].iter().rev().cloned().collect() };
        return Some(slice);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadKind {
    Incisor,
    Canine,
    Premolar,
    Molar,
    Carnassial,
}

struct Head {
    kind: HeadKind,
    ordinal: Option<u8>,
    plural: bool,
    start: usize,
    end: usize,
}

/// "P4", "PM4", "M1", "I3".
fn compact(w: &str) -> Option<(HeadKind, u8)> {
    let digit = w.chars().last()?.to_digit(10)? as u8;
    let kind = match &w[..w.len() - 1] {
        "p" | "pm" => HeadKind::Premolar,
        "m" => HeadKind::Molar,
        "i" => HeadKind::Incisor,
        _ => return None,
    };
    (1..=4).contains(&digit).then_some((kind, digit))
}

fn small_number(w: &str) -> Option<u8> {
    match w {
        "one" | "1" => Some(1),
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        _ => None,
    }
}

/// A tooth word at `i`: "canine", "premolars", "pre-molar", "P4", "P four",
/// "canine teeth".
fn head_at(toks: &[Tok], i: usize) -> Option<Head> {
    let w = toks[i].w.as_str();
    let next = toks.get(i + 1).filter(|t| !t.brk).map(|t| t.w.as_str());
    let mut start = i;
    let mut end = i + 1;
    let mut ordinal = None;
    let (kind, mut plural) = match w {
        "incisor" => (HeadKind::Incisor, false),
        "incisors" => (HeadKind::Incisor, true),
        "canine" | "fang" | "cuspid" => (HeadKind::Canine, false),
        "canines" | "fangs" | "cuspids" => (HeadKind::Canine, true),
        // The recogniser's spellings too: "108 upper left 4th primolar".
        "premolar" | "primolar" | "premoler" => (HeadKind::Premolar, false),
        "premolars" | "primolars" | "premolers" => (HeadKind::Premolar, true),
        "molar" | "molars" => {
            let pre = i > 0 && toks[i - 1].w == "pre" && !toks[i].brk;
            if pre {
                start = i - 1;
            }
            (if pre { HeadKind::Premolar } else { HeadKind::Molar }, w == "molars")
        }
        "carnassial" => (HeadKind::Carnassial, false),
        "carnassials" => (HeadKind::Carnassial, true),
        "p" | "pm" | "m" => {
            let n = next.and_then(small_number)?;
            end = i + 2;
            ordinal = Some(n);
            (if w == "m" { HeadKind::Molar } else { HeadKind::Premolar }, false)
        }
        _ => {
            let (kind, n) = compact(w)?;
            ordinal = Some(n);
            (kind, false)
        }
    };
    match toks.get(end).filter(|t| !t.brk).map(|t| t.w.as_str()) {
        Some("teeth") => {
            plural = true;
            end += 1;
        }
        Some("tooth") => {
            plural = false;
            end += 1;
        }
        _ => {}
    }
    Some(Head { kind, ordinal, plural, start, end })
}

#[derive(Default, Clone)]
struct Mods {
    right: bool,
    left: bool,
    upper: bool,
    lower: bool,
    deciduous: bool,
    permanent: bool,
    ordinals: Vec<u8>,
}

impl Mods {
    fn has_place(&self) -> bool {
        self.right || self.left || self.upper || self.lower
    }

    fn set(&mut self, m: &Modifier) {
        match m {
            Modifier::Side(Side::Right) => self.right = true,
            Modifier::Side(Side::Left) => self.left = true,
            Modifier::Jaw(Jaw::Upper) => self.upper = true,
            Modifier::Jaw(Jaw::Lower) => self.lower = true,
            Modifier::Deciduous => self.deciduous = true,
            Modifier::Permanent => self.permanent = true,
            Modifier::Ordinal(o) | Modifier::Position(o) => self.ordinals.push(*o),
        }
    }
}

/// Words before a group that say how much of it is meant.
#[derive(Default, Clone, Copy)]
struct Quant {
    /// "all", "every", "each": the whole group.
    all: bool,
    /// "both": a group of two.
    both: bool,
    /// "the four canines": exactly that many.
    count: Option<usize>,
    /// "no", "none of", "not", "neither": the finding is said to be absent.
    negated: bool,
    /// "some", "a few", "two of", "the other", "the remaining": part of the
    /// group, and which part isn't said.
    partial: bool,
}

fn quantifier(w: &str, q: &mut Quant) -> bool {
    match w {
        "all" | "every" | "each" | "entire" => q.all = true,
        "both" => q.both = true,
        "no" | "none" | "not" | "neither" | "nor" | "never" | "without" => q.negated = true,
        "some" | "few" | "couple" | "several" | "most" | "many" | "any" | "either" | "other" | "others"
        | "another" | "remaining" | "rest" | "else" | "half" | "multiple" | "various" | "certain" => {
            q.partial = true
        }
        _ => match count_word(w) {
            Some(n) => q.count = Some(n),
            None => return false,
        },
    }
    true
}

fn count_word(w: &str) -> Option<usize> {
    Some(match w {
        "one" | "1" => 1,
        "two" | "2" => 2,
        "three" | "3" => 3,
        "four" | "4" => 4,
        "five" | "5" => 5,
        "six" | "6" => 6,
        "seven" | "7" => 7,
        "eight" | "8" => 8,
        "nine" | "9" => 9,
        "ten" | "10" => 10,
        "eleven" | "11" => 11,
        "twelve" | "12" => 12,
        _ => return None,
    })
}

/// Words that may follow a tooth word before its side or jaw: "the fourth
/// premolar on the upper right", "first molar, lower jaw".
const AFTER_HEAD: &[&str] =
    &["on", "in", "of", "at", "the", "side", "jaw", "arcade", "arch", "quadrant", "mouth"];

/// Words that may sit between two tooth names said together, so the second
/// takes the first's side and jaw: "lower left first molar and second molar".
const NAME_JOIN: &[&str] = &["and", "or", "the", "its", "his", "her", "their", "also", "plus"];

enum Modifier {
    Side(Side),
    Jaw(Jaw),
    Deciduous,
    Permanent,
    Ordinal(u8),
    /// Incisor position: central 1, intermediate 2, corner 3.
    Position(u8),
}

fn modifier(w: &str) -> Option<Modifier> {
    Some(match w {
        "right" => Modifier::Side(Side::Right),
        "left" => Modifier::Side(Side::Left),
        "upper" | "maxillary" | "maxilla" | "top" => Modifier::Jaw(Jaw::Upper),
        "lower" | "mandibular" | "mandible" | "bottom" => Modifier::Jaw(Jaw::Lower),
        "deciduous" | "baby" | "milk" | "primary" => Modifier::Deciduous,
        "adult" | "permanent" => Modifier::Permanent,
        "first" | "1st" => Modifier::Ordinal(1),
        "second" | "2nd" => Modifier::Ordinal(2),
        "third" | "3rd" => Modifier::Ordinal(3),
        "fourth" | "4th" => Modifier::Ordinal(4),
        "central" => Modifier::Position(1),
        "intermediate" | "middle" => Modifier::Position(2),
        "corner" | "lateral" => Modifier::Position(3),
        _ => return None,
    })
}

/// Words that may sit between a tooth's name parts: "the upper left
/// fractured canine". Counts and "all", "both", "some" are read as
/// quantifiers instead ([`Quant`]).
#[rustfmt::skip]
const NAME_FILLER: &[&str] = &[
    "the", "a", "an", "of", "his", "her", "its", "their", "and",
    "persistent", "retained", "fractured", "broken", "missing", "mobile", "loose", "diseased", "affected",
    "erupted", "unerupted", "worn", "discoloured", "discolored",
];

/// The teeth a name or group means, or None when it is ambiguous.
///
/// A group needs a side or a jaw ("the lower incisors"), or a word for the
/// whole of it ("all the canines"). A count or a vague amount in front of it
/// ("two of the lower incisors", "a few premolars") or a negation ("none of
/// the canines", "no fractured canines") names no tooth, because which teeth
/// are meant isn't said; so does "the other premolars".
fn resolve(head: &Head, m: &Mods, q: Quant) -> Option<(Vec<String>, SaidAs)> {
    if m.deciduous && m.permanent {
        return None;
    }
    // "each canine", "every premolar": the group.
    let plural = head.plural || q.all;
    if !plural {
        if q.partial {
            return None; // "either lower canine", "the other upper canine"
        }
    } else {
        if q.negated || q.partial {
            return None;
        }
        // "upper right and lower left canines": both sides and both jaws said.
        if m.right && m.left && m.upper && m.lower {
            return None;
        }
        if !m.has_place() && !q.all && !q.both && q.count.is_none() {
            return None; // "calculus on the premolars"
        }
    }
    let deciduous = m.deciduous;
    let mut ordinals = m.ordinals.clone();
    if let Some(o) = head.ordinal {
        ordinals.push(o);
    }
    ordinals.sort_unstable();
    ordinals.dedup();
    let sides: Vec<Side> = match (m.right, m.left) {
        (true, false) => vec![Side::Right],
        (false, true) => vec![Side::Left],
        _ => vec![Side::Right, Side::Left],
    };
    let jaws: Vec<Jaw> = match (m.upper, m.lower) {
        (true, false) => vec![Jaw::Upper],
        (false, true) => vec![Jaw::Lower],
        _ => vec![Jaw::Upper, Jaw::Lower],
    };
    if !plural {
        // One tooth: the side and the jaw must both be said, once each.
        if m.right == m.left || m.upper == m.lower || ordinals.len() > 1 {
            return None;
        }
        let ordinal = ordinals.first().copied();
        let position: u8 = match (head.kind, ordinal) {
            (HeadKind::Incisor, Some(o @ 1..=3)) => o,
            (HeadKind::Canine, None) => 4,
            (HeadKind::Premolar, Some(o @ 1..=4)) => o + 4,
            (HeadKind::Molar, Some(o @ 1..=3)) if !deciduous => o + 8,
            (HeadKind::Carnassial, None) if !deciduous => {
                if jaws[0] == Jaw::Upper {
                    8
                } else {
                    9
                }
            }
            _ => return None,
        };
        let quadrant = match (jaws[0], sides[0]) {
            (Jaw::Upper, Side::Right) => 1,
            (Jaw::Upper, Side::Left) => 2,
            (Jaw::Lower, Side::Left) => 3,
            (Jaw::Lower, Side::Right) => 4,
        } + if deciduous { 4 } else { 0 };
        let id = format!("{quadrant}{position:02}");
        return tooth(&id).map(|t| (vec![t.id.clone()], SaidAs::Name));
    }
    let teeth: Vec<String> = chart()
        .teeth
        .iter()
        .filter(|t| t.deciduous == deciduous && sides.contains(&t.side) && jaws.contains(&t.jaw))
        .filter(|t| {
            let by_ordinal = ordinals.is_empty() || ordinals.contains(&t.ordinal);
            match head.kind {
                HeadKind::Incisor => t.kind == ToothType::Incisor && by_ordinal,
                HeadKind::Canine => t.kind == ToothType::Canine,
                HeadKind::Premolar => t.kind == ToothType::Premolar && by_ordinal,
                HeadKind::Molar => t.kind == ToothType::Molar && by_ordinal,
                HeadKind::Carnassial => t.shape == Shape::Carnassial,
            }
        })
        .map(|t| t.id.clone())
        .collect();
    // "both" is two teeth; a count is that many (in the dog's mouth).
    let wanted = q.count.or(q.both.then_some(2));
    if wanted.is_some_and(|n| n != teeth.len()) {
        return None;
    }
    (!teeth.is_empty()).then_some((teeth, SaidAs::Group))
}

/// Every tooth said in `text`, by number, name, range or group, in order.
/// Only valid Triadan ids count; an ambiguous name ("the canine", "a
/// premolar") gives no tooth, and a number followed by a unit or a time ("a
/// 108 mg tablet", "three oh nine pm") isn't a tooth.
pub fn teeth_in(text: &str) -> Vec<Mention> {
    let toks = tokens(text);
    let mut used = vec![false; toks.len()];
    let mut numbers: Vec<Mention> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Some((n, len)) = number_at(&toks, i) {
            let id = n.to_string();
            if tooth(&id).is_some() && !not_a_tooth(&toks, i, len) {
                numbers.push(Mention { teeth: vec![id], said_as: SaidAs::Number, words: (i, i + len) });
                used[i..i + len].iter_mut().for_each(|u| *u = true);
                i += len;
                continue;
            }
        }
        i += 1;
    }

    // Ranges: "101 through 203" along one arcade; "106 to 108" and "106-108"
    // only inside one quadrant ("from 108 to 208" is two teeth, not sixteen).
    let mut mentions: Vec<Mention> = Vec::new();
    let mut k = 0;
    while k < numbers.len() {
        if let Some(b) = numbers.get(k + 1) {
            let a = &numbers[k];
            let gap = b.words.0 - a.words.1;
            let link = (gap == 1 && !toks[a.words.1].brk).then(|| toks[a.words.1].w.as_str());
            let through = link.is_some_and(|w| RANGE_THROUGH.contains(&w));
            let to = link.is_some_and(|w| RANGE_TO.contains(&w)) || (gap == 0 && toks[b.words.0].hyphen);
            let one_quadrant = a.teeth[0].as_bytes()[0] == b.teeth[0].as_bytes()[0];
            let joined = !toks[b.words.0].brk && (through || (to && one_quadrant));
            if joined && let Some(teeth) = arcade(&a.teeth[0], &b.teeth[0]) {
                if gap == 1 {
                    used[a.words.1] = true;
                }
                mentions.push(Mention { teeth, said_as: SaidAs::Range, words: (a.words.0, b.words.1) });
                k += 2;
                continue;
            }
        }
        mentions.push(numbers[k].clone());
        k += 1;
    }

    // Names and groups. The last name read, with its side and jaw, for a
    // second name said with it ("lower left first molar and second molar").
    let mut last_name: Option<(usize, Mods)> = None;
    let mut i = 0;
    while i < toks.len() {
        if used[i] {
            i += 1;
            continue;
        }
        let Some(head) = head_at(&toks, i) else {
            i += 1;
            continue;
        };
        let mut mods = Mods::default();
        let mut quant = Quant::default();
        let mut start = head.start;
        let mut j = head.start;
        while j > 0 && head.start - j < 7 {
            if toks[j].brk || used[j - 1] {
                break;
            }
            let w = toks[j - 1].w.as_str();
            match modifier(w) {
                Some(Modifier::Position(_)) if head.kind != HeadKind::Incisor => break,
                Some(m) => mods.set(&m),
                None if quantifier(w, &mut quant) => {}
                None if NAME_FILLER.contains(&w) => {
                    j -= 1;
                    continue;
                }
                None => break,
            }
            j -= 1;
            start = j;
        }
        // Side and jaw after the tooth word: "the fourth premolar on the
        // upper right", "first molar, lower left". Not when they lead into
        // the next tooth's name.
        let mut end = head.end;
        let mut after = Mods::default();
        let mut k = head.end;
        while k < toks.len()
            && k < head.end + 6
            && !used[k]
            && !toks[k].stop
            && (k == head.end || !toks[k].brk)
        {
            let w = toks[k].w.as_str();
            match modifier(w) {
                Some(m @ (Modifier::Side(_) | Modifier::Jaw(_))) => {
                    after.set(&m);
                    end = k + 1;
                }
                None if AFTER_HEAD.contains(&w) => {}
                _ => break,
            }
            k += 1;
        }
        let mut next = end;
        while next < toks.len()
            && !toks[next].brk
            && (NAME_FILLER.contains(&toks[next].w.as_str())
                || matches!(
                    modifier(&toks[next].w),
                    Some(
                        Modifier::Ordinal(_)
                            | Modifier::Position(_)
                            | Modifier::Deciduous
                            | Modifier::Permanent
                    )
                ))
        {
            next += 1;
        }
        let leads_on =
            end > head.end && next < toks.len() && !toks[next].brk && head_at(&toks, next).is_some();
        if end > head.end && !leads_on {
            mods.right |= after.right;
            mods.left |= after.left;
            mods.upper |= after.upper;
            mods.lower |= after.lower;
        } else {
            end = head.end;
        }
        // A second name said with the last one takes its side and jaw when
        // it has neither: "the upper right canine and premolars".
        if !mods.has_place()
            && let Some((last_end, last)) = &last_name
            && *last_end <= start
            && (*last_end..start).all(|x| NAME_JOIN.contains(&toks[x].w.as_str()))
            && (*last_end..=start).all(|x| !toks[x].stop)
        {
            mods.right = last.right;
            mods.left = last.left;
            mods.upper = last.upper;
            mods.lower = last.lower;
            mods.deciduous |= last.deciduous && !mods.permanent;
        }
        if let Some((teeth, said_as)) = resolve(&head, &mods, quant) {
            used[start..end].iter_mut().for_each(|u| *u = true);
            mentions.push(Mention { teeth, said_as, words: (start, end) });
            last_name = Some((end, mods));
        }
        i = end;
    }
    mentions.sort_by_key(|m| m.words);
    mentions
}

// ---------------------------------------------------------------------------
// Faithfulness for teeth
// ---------------------------------------------------------------------------

/// The kinds of flag a charted tooth can get. All are soft (the vet looks
/// before signing) except [`ToothFlagKind::blocks_signing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToothFlagKind {
    /// A number and a name next to it mean different teeth ("108, upper left
    /// fourth premolar"), whichever of the two was charted. Never corrected
    /// automatically.
    NameConflict,
    /// The tooth's clause has no word for the finding or its type.
    CueMissing,
    /// The words for the finding, its type, its grade or its millimetres are
    /// in the cited line, but said about another tooth ("309 has grade one
    /// mobility and 409 is grade three", charted as 309 grade 3).
    OtherTooth,
    /// The line says the finding, or its type, wasn't there: "no resorption",
    /// "not fractured", "uncomplicated" for a complicated fracture.
    Negated,
    /// The tooth's clause also has the words for another type of the same
    /// finding ("surgically with a flap" on a closed extraction).
    OtherType,
    /// Charted as done, but the line sounds like a plan ("may need to come
    /// out").
    Planned,
    /// Findings on one tooth that can't both be true: normal beside a
    /// fracture, two resorption stages, two extraction techniques. `other` is
    /// the other item's path.
    Conflict,
    /// `detail`, `grade` or `mm` don't fit the finding, or the finding doesn't
    /// fit the kind of tooth (a persistent deciduous tooth on 104).
    Shape,
    /// The same finding twice on one tooth.
    Duplicate,
    /// The tooth isn't in the chart's dentition (a 105 in a cat). Blocks
    /// signing.
    NotInMouth,
    /// No dentition is recorded, and only a dog has this tooth. Blocks
    /// signing until dog or cat is chosen.
    ChooseDentition,
}

impl ToothFlagKind {
    /// Flags that block signing, like a number said nowhere.
    pub fn blocks_signing(self) -> bool {
        matches!(self, ToothFlagKind::NotInMouth | ToothFlagKind::ChooseDentition)
    }
}

/// A flag on a charted tooth. Soft flags never block signing; see
/// [`ToothFlagKind::blocks_signing`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToothFlag {
    /// JSON pointer to the item: "/dental_chart/3".
    pub path: String,
    pub tooth: String,
    pub kind: ToothFlagKind,
    /// Name conflict: the other tooth (the one not charted). Conflict: the
    /// other item's path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    /// Cue missing, negated, other tooth: the words looked for. Other type:
    /// the other type's words. Shape: the reason. Conflict: the other item's
    /// printed words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected: Vec<String>,
}

/// What the tooth checks found, in the faithfulness report's own types so the
/// pass can merge it: hard fails go to `unsupported_numbers` (with the tooth
/// as the number), wrong citations to `citation_mismatches`, second-recogniser
/// disagreements to `disputed_numbers`, and flags to `tooth_flags`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToothReport {
    /// Items checked (vet-entered items are listed by the caller, not here).
    pub checked: u32,
    pub unsupported: Vec<UnsupportedNumber>,
    pub mismatches: Vec<UnsupportedNumber>,
    pub disputed: Vec<DisputedNumber>,
    pub flags: Vec<ToothFlag>,
}

impl ToothReport {
    fn merge(&mut self, other: ToothReport) {
        self.checked += other.checked;
        self.unsupported.extend(other.unsupported);
        self.mismatches.extend(other.mismatches);
        self.disputed.extend(other.disputed);
        self.flags.extend(other.flags);
    }

    fn flag(
        &mut self,
        path: &str,
        tooth: &str,
        kind: ToothFlagKind,
        other: Option<String>,
        expected: Vec<String>,
    ) {
        if self.flags.iter().any(|f| f.path == path && f.kind == kind) {
            return; // one flag of a kind per item
        }
        self.flags.push(ToothFlag {
            path: path.to_string(),
            tooth: tooth.to_string(),
            kind,
            other,
            expected,
        });
    }
}

/// A crude English stem for cue words, as in the claim alignment: plurals and
/// common verb endings ("extracting" and "extracted" are "extract").
fn stem(w: &str) -> String {
    let w = w.strip_suffix("'s").unwrap_or(w);
    for (suffix, min, replace) in [
        ("ies", 4, "y"),
        ("ing", 5, ""),
        ("edly", 6, ""),
        ("ed", 4, ""),
        ("es", 4, ""),
        ("ly", 5, ""),
        ("s", 3, ""),
    ] {
        if w.len() > min
            && let Some(base) = w.strip_suffix(suffix)
        {
            if suffix == "s" && (base.ends_with('s') || base.ends_with('u') || base.ends_with('i')) {
                return w.to_string();
            }
            let b = base.as_bytes();
            if (suffix == "ing" || suffix == "ed")
                && b.len() >= 3
                && b[b.len() - 1] == b[b.len() - 2]
                && !b"aeioulsfz".contains(&b[b.len() - 1])
            {
                return base[..base.len() - 1].to_string();
            }
            return format!("{base}{replace}");
        }
    }
    w.to_string()
}

/// Words that start a negation: "no pulp exposure", "not fractured",
/// "without sectioning", "doesn't need to come out".
const NEGATORS: &[&str] = &["no", "not", "without", "never", "none", "nor", "neither"];

/// Words that end a negation before punctuation does: "not mobile but
/// fractured".
const NEGATION_ENDS: &[&str] =
    &["but", "and", "although", "though", "however", "except", "which", "while", "whereas"];

/// Which words of a line are inside a negation. Forward from "no", "not",
/// "without", "never", "none", "negative for" and "free of" to the next
/// punctuation, tooth or [`NEGATION_ENDS`] word; backward from "ruled out"
/// and "absent" to the clause start or a tooth. `absent_negates`: false for
/// the `missing` finding, whose own word "absent" is.
fn negated(toks: &[Tok], mentions: &[Mention], absent_negates: bool) -> Vec<bool> {
    let in_mention = |i: usize| mentions.iter().any(|m| (m.words.0..m.words.1).contains(&i));
    let mut out = vec![false; toks.len()];
    for i in 0..toks.len() {
        let w = toks[i].w.as_str();
        let next = toks.get(i + 1).filter(|t| !t.brk).map(|t| t.w.as_str());
        let forward =
            NEGATORS.contains(&w) || (matches!(w, "negative" | "free") && matches!(next, Some("for" | "of")));
        if forward && !in_mention(i) {
            let mut k = i + 1;
            while k < toks.len()
                && !toks[k].brk
                && !in_mention(k)
                && !NEGATION_ENDS.contains(&toks[k].w.as_str())
            {
                out[k] = true;
                k += 1;
            }
        }
        let ruled_out = w == "out" && i > 0 && toks[i - 1].w == "ruled" && !toks[i].brk;
        let backward = ruled_out || (absent_negates && w == "absent");
        if backward {
            let mut k = if ruled_out { i - 1 } else { i };
            while k > 0 && !toks[k].brk {
                k -= 1;
                if in_mention(k) || NEGATION_ENDS.contains(&toks[k].w.as_str()) {
                    break;
                }
                out[k] = true;
            }
        }
    }
    out
}

/// "closed" meaning a sutured flap or site, not a closed extraction: "closed
/// with 4-0 Monocryl", "the flap was closed".
fn suture_closed(toks: &[Tok], i: usize) -> bool {
    if toks[i].w != "closed" {
        return false;
    }
    let next = toks.get(i + 1).filter(|t| !t.brk).map(|t| t.w.as_str());
    if matches!(next, Some("with" | "using" | "over" | "in" | "primarily")) {
        return true;
    }
    let mut k = i;
    while k > 0 && !toks[k].brk {
        k -= 1;
        match toks[k].w.as_str() {
            "was" | "were" | "is" | "then" | "and" | "been" if k + 3 > i => continue,
            "flap" | "flaps" | "site" | "sites" | "incision" | "gingiva" | "gum" | "gums" | "defect"
            | "socket" | "sockets" | "wound" | "sutured" => return true,
            _ => return false,
        }
    }
    false
}

/// The words a check reads: stems, and whether each is negated or masked.
struct Words {
    stems: Vec<String>,
    neg: Vec<bool>,
    masked: Vec<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    Yes,
    /// Said only inside a negation.
    Negated,
    No,
}

/// Is the cue (a word or a short phrase) said in these runs of words? A cue
/// with its own negation ("no pulp", "not erupted") matches as written; any
/// other cue counts only outside a negation.
fn said_in(runs: &[Words], cue: &str) -> Said {
    let cue_words = tokens(cue);
    let cue_stems: Vec<String> = cue_words.iter().map(|t| stem(&t.w)).collect();
    if cue_stems.is_empty() {
        return Said::No;
    }
    let own_negation = cue_words.iter().any(|t| NEGATORS.contains(&t.w.as_str()));
    let mut negated_only = false;
    for run in runs {
        for at in 0..run.stems.len().saturating_sub(cue_stems.len() - 1) {
            let span = at..at + cue_stems.len();
            if run.stems[span.clone()] != cue_stems[..] || run.masked[span.clone()].iter().any(|m| *m) {
                continue;
            }
            if own_negation || !run.neg[span].iter().any(|n| *n) {
                return Said::Yes;
            }
            negated_only = true;
        }
    }
    if negated_only { Said::Negated } else { Said::No }
}

/// The best of several cues: said, said only negated, or not said.
fn any_said(runs: &[Words], cues: &[String]) -> Said {
    let mut best = Said::No;
    for c in cues {
        match said_in(runs, c) {
            Said::Yes => return Said::Yes,
            Said::Negated => best = Said::Negated,
            Said::No => {}
        }
    }
    best
}

/// Mentions said together as one subject: "five oh four and six oh four",
/// "108, the upper right fourth premolar", "108, which is the …".
#[rustfmt::skip]
const SUBJECT_JOIN: &[&str] = &[
    "and", "or", "the", "a", "an", "plus", "also", "as", "well", "which", "is", "that", "that's", "aka",
    "known", "called", "being", "tooth", "teeth", "its", "it's",
];

/// Words that start a new clause between two subjects of one sentence.
const CLAUSE_WORDS: &[&str] =
    &["and", "but", "while", "whereas", "then", "although", "though", "however", "except"];

/// A line read for the tooth checks: its words, the teeth in it, and for a
/// tooth the clause that speaks about it.
struct Line {
    chars: Vec<char>,
    toks: Vec<Tok>,
    mentions: Vec<Mention>,
}

impl Line {
    fn new(text: &str) -> Self {
        Line { chars: chars_of(text), toks: tokens(text), mentions: teeth_in(text) }
    }

    /// The sentence index of each word.
    fn sentences(&self) -> Vec<usize> {
        let mut n = 0;
        self.toks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i > 0 && t.stop {
                    n += 1;
                }
                n
            })
            .collect()
    }

    /// Mentions grouped into subjects: (first word, end, mention indexes).
    fn subjects(&self, sent: &[usize]) -> Vec<(usize, usize, Vec<usize>)> {
        let mut out: Vec<(usize, usize, Vec<usize>)> = Vec::new();
        for (n, m) in self.mentions.iter().enumerate() {
            if let Some(last) = out.last_mut()
                && sent[last.0] == sent[m.words.0.min(self.toks.len() - 1)]
                && (last.1..m.words.0).all(|x| SUBJECT_JOIN.contains(&self.toks[x].w.as_str()))
            {
                last.1 = last.1.max(m.words.1);
                last.2.push(n);
                continue;
            }
            out.push((m.words.0, m.words.1, vec![n]));
        }
        out
    }

    /// The clauses about `tooth`, as word ranges: for each subject that names
    /// it, from the start of its sentence (or the clause break after the
    /// subject before it) to the end of its sentence (or the last clause break
    /// before the next subject). With no break between two subjects, the
    /// words between belong to the first.
    fn windows(&self, tooth: &str) -> Vec<(usize, usize)> {
        if self.toks.is_empty() {
            return Vec::new();
        }
        let sent = self.sentences();
        let subjects = self.subjects(&sent);
        let first_of = |s: usize| sent.iter().position(|x| *x == s).unwrap_or(0);
        let end_of = |s: usize| sent.iter().rposition(|x| *x == s).map_or(0, |i| i + 1);
        // The last clause break between two subjects: (end of the first's
        // clause, start of the second's).
        let split = |from: usize, to: usize| -> (usize, usize) {
            let mut at = (to, to);
            for x in from..=to.min(self.toks.len() - 1) {
                if self.toks[x].brk && x > from {
                    at = (x, x);
                }
                if x < to && CLAUSE_WORDS.contains(&self.toks[x].w.as_str()) {
                    at = (x, x + 1);
                }
            }
            at
        };
        let mut out = Vec::new();
        for (n, (start, end, members)) in subjects.iter().enumerate() {
            if !members.iter().any(|m| self.mentions[*m].covers(tooth)) {
                continue;
            }
            let s = sent[*start];
            let from = match n.checked_sub(1).map(|p| &subjects[p]) {
                Some(prev) if sent[prev.0] == s => split(prev.1, *start).1,
                _ => first_of(s),
            };
            let to = match subjects.get(n + 1) {
                Some(next) if sent[next.0] == s => split(*end, next.0).0,
                _ => end_of(s),
            };
            out.push((from.min(*start), to.max(*end)));
        }
        out
    }

    /// The words in these ranges, each run read with its negations.
    fn words(&self, ranges: &[(usize, usize)], absent_negates: bool) -> Vec<Words> {
        let neg = negated(&self.toks, &self.mentions, absent_negates);
        ranges
            .iter()
            .map(|&(a, b)| Words {
                stems: self.toks[a..b].iter().map(|t| stem(&t.w)).collect(),
                neg: neg[a..b].to_vec(),
                masked: (a..b).map(|i| suture_closed(&self.toks, i)).collect(),
            })
            .collect()
    }

    /// The word ranges not in `ranges`.
    fn outside(&self, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = 0;
        let mut sorted = ranges.to_vec();
        sorted.sort_unstable();
        for (a, b) in sorted {
            if a > at {
                out.push((at, a));
            }
            at = at.max(b);
        }
        if at < self.toks.len() {
            out.push((at, self.toks.len()));
        }
        out
    }

    /// The words of the sentences these ranges sit in, outside the ranges.
    fn beside(&self, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
        let sent = self.sentences();
        let mine: HashSet<usize> =
            ranges.iter().filter(|(a, _)| *a < sent.len()).map(|(a, _)| sent[*a]).collect();
        let mut out = Vec::new();
        for (a, b) in self.outside(ranges) {
            let mut run: Option<usize> = None;
            for i in a..=b {
                let keep = i < b && sent.get(i).is_some_and(|s| mine.contains(s));
                match (keep, run) {
                    (true, None) => run = Some(i),
                    (false, Some(s)) => {
                        out.push((s, i));
                        run = None;
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// Numbers said in these ranges, outside the tooth mentions themselves
    /// (so "three oh nine" never supports a grade of 3, nor "fourth premolar"
    /// a stage 4).
    fn numbers(&self, ranges: &[(usize, usize)]) -> HashSet<String> {
        let in_mention = |i: usize| self.mentions.iter().any(|m| (m.words.0..m.words.1).contains(&i));
        let mut runs: Vec<String> = Vec::new();
        for &(a, b) in ranges {
            let mut run: Option<(usize, usize)> = None;
            for i in a..=b {
                let keep = i < b && !in_mention(i);
                match (keep, run) {
                    (true, None) => run = Some((i, i)),
                    (true, Some((s, _))) => run = Some((s, i)),
                    (false, Some((s, e))) => {
                        runs.push(self.chars[self.toks[s].span.0..self.toks[e].span.1].iter().collect());
                        run = None;
                    }
                    (false, None) => {}
                }
            }
        }
        runs.iter().flat_map(|r| extract_numbers(&normalize_spoken(r))).collect()
    }
}

/// Plan or possibility words: "may need to come out", "we'll extract it".
#[rustfmt::skip]
const PLAN_WORDS: &[&str] = &[
    "may", "might", "will", "would", "could", "should", "plan", "plans", "planned", "planning", "recommend",
    "recommended", "recommending", "consider", "considering", "if", "possibly", "probably", "schedule",
    "scheduled", "later", "eventually",
];

/// Treatment said as done or being done: "extracted", "extracting", "came out".
#[rustfmt::skip]
const DONE_WORDS: &[&str] = &[
    "extracted", "extracting", "removed", "removing", "came", "pulled", "pulling", "amputated", "amputating",
    "sectioned", "elevated", "done", "performed", "took",
];

/// Does the clause sound like a plan, with no word for it being done?
fn sounds_planned(line: &Line, ranges: &[(usize, usize)]) -> bool {
    let words: Vec<&str> =
        ranges.iter().flat_map(|&(a, b)| line.toks[a..b].iter().map(|t| t.w.as_str())).collect();
    let plan = words.iter().enumerate().any(|(i, w)| {
        PLAN_WORDS.contains(w)
            || w.ends_with("'ll")
            || (matches!(*w, "need" | "needs" | "going" | "want" | "wants")
                && words.get(i + 1) == Some(&"to"))
    });
    plan && !words.iter().any(|w| DONE_WORDS.contains(w))
}

/// Words that may sit between a tooth's number and its name when one restates
/// the other.
#[rustfmt::skip]
const APPOSITION: &[&str] = &[
    "the", "a", "an", "that", "that's", "is", "which", "or", "aka", "also", "known", "as", "called", "being",
    "tooth",
];

/// Numbers with a name beside them, in apposition, that mean another tooth:
/// "108, upper left fourth premolar", "108, which is the upper left fourth
/// premolar". Not "206 is missing, and the upper left canine looks healthy",
/// and not when the name's own number was said too. (number's tooth, name's
/// tooth) pairs.
fn disagreeing_names(toks: &[Tok], line: &[Mention]) -> Vec<(String, String)> {
    let numbers = || line.iter().filter(|m| m.said_as == SaidAs::Number);
    line.iter()
        .filter(|m| m.said_as == SaidAs::Name && m.teeth.len() == 1)
        .filter_map(|name| {
            let named = &name.teeth[0];
            if numbers().any(|m| m.covers(named)) {
                return None; // its own number was said too
            }
            let nearest = numbers().filter(|m| m.gap(name) <= 6).min_by_key(|m| m.gap(name))?;
            let (from, to) = if nearest.words.1 <= name.words.0 {
                (nearest.words.1, name.words.0)
            } else {
                (name.words.1, nearest.words.0)
            };
            let apposition = toks[from..to].iter().all(|t| APPOSITION.contains(&t.w.as_str()));
            (apposition && nearest.teeth[0] != *named).then(|| (nearest.teeth[0].clone(), named.clone()))
        })
        .collect()
}

/// The teeth for a line's prompt hint (`note::with_tooth_hints`): every
/// tooth read in it, except a name that disagrees with the number beside it,
/// so the hint matches the rule to chart the number as said.
pub fn hint_teeth(text: &str) -> Vec<String> {
    let toks = tokens(text);
    let line = teeth_in(text);
    let wrong: Vec<String> = disagreeing_names(&toks, &line).into_iter().map(|(_, name)| name).collect();
    let mut out: Vec<String> = Vec::new();
    for m in &line {
        for t in &m.teeth {
            let named_wrongly = m.said_as == SaidAs::Name && wrong.contains(t);
            if !named_wrongly && !out.contains(t) {
                out.push(t.clone());
            }
        }
    }
    out
}

/// The clauses of `text` that speak about `tooth` (for tests and tools).
pub fn tooth_clauses(text: &str, tooth: &str) -> Vec<String> {
    let line = Line::new(text);
    line.windows(tooth)
        .into_iter()
        .map(|(a, b)| line.chars[line.toks[a].span.0..line.toks[b - 1].span.1].iter().collect())
        .collect()
}

fn is_vet_entered(map: &Map<String, Value>) -> bool {
    map.get("entered_by").and_then(Value::as_str).is_some_and(|b| !b.is_empty())
}

/// Check one charted finding against its cited lines (ADR-0021 decision 7).
/// Numbers in `text`, `grade` and `mm` stay with the generic number check,
/// which skips the `tooth` key; this check replaces it for the tooth, and
/// adds where in the line the finding, grade and millimetres were said.
///
/// `dentition`: the chart's, or None when it isn't recorded.
pub fn check_item(
    map: &Map<String, Value>,
    path: &str,
    transcript: &Transcript,
    dentition: Option<Dentition>,
) -> ToothReport {
    let mut report = ToothReport::default();
    let Some(tooth_id) = map.get("tooth").and_then(Value::as_str) else { return report };
    // The mouth first, for the vet's own items too.
    match (dentition, tooth(tooth_id)) {
        (Some(d), Some(t)) if !t.dentitions.contains(&d) => {
            report.flag(path, tooth_id, ToothFlagKind::NotInMouth, None, vec![d.animal().to_string()]);
        }
        (None, Some(t)) if !t.dentitions.contains(&Dentition::Feline) => {
            report.flag(path, tooth_id, ToothFlagKind::ChooseDentition, None, Vec::new());
        }
        _ => {}
    }
    if is_vet_entered(map) {
        return report;
    }
    report.checked = 1;
    let item = Value::Object(map.clone());
    let cited: Vec<u32> = map
        .get("evidence")
        .and_then(Value::as_array)
        .map(|e| e.iter().filter_map(Value::as_u64).map(|v| v as u32).collect())
        .unwrap_or_default();
    let segments: Vec<&Segment> = cited.iter().filter_map(|id| transcript.get(*id)).collect();
    let lines: Vec<Line> = segments.iter().map(|s| Line::new(&s.text)).collect();

    // Said in the cited lines (by number, name, range or group), elsewhere by
    // number, name or range, or nowhere. A group said elsewhere ("calculus on
    // the premolars") never names a tooth for this.
    if !lines.iter().flat_map(|l| &l.mentions).any(|m| m.covers(tooth_id)) {
        let found_in: Vec<u32> = transcript
            .segments
            .iter()
            .filter(|s| teeth_in(&s.text).iter().any(|m| m.said_as != SaidAs::Group && m.covers(tooth_id)))
            .map(|s| s.id)
            .collect();
        let entry = UnsupportedNumber {
            path: path.to_string(),
            number: tooth_id.to_string(),
            cited: cited.clone(),
            found_in,
        };
        if entry.found_in.is_empty() {
            report.unsupported.push(entry);
        } else {
            report.mismatches.push(entry);
        }
    }

    // A number and a name beside it that mean different teeth, whichever of
    // the two was charted.
    let conflict = lines.iter().find_map(|line| {
        disagreeing_names(&line.toks, &line.mentions).into_iter().find_map(|(number, name)| {
            if number == tooth_id {
                Some(name)
            } else if name == tooth_id {
                Some(number)
            } else {
                None
            }
        })
    });
    if let Some(other) = conflict {
        report.flag(path, tooth_id, ToothFlagKind::NameConflict, Some(other), Vec::new());
    }

    // The second recogniser heard another number where this tooth was said.
    for (seg, line) in segments.iter().zip(&lines) {
        let by_number = line
            .mentions
            .iter()
            .any(|m| matches!(m.said_as, SaidAs::Number | SaidAs::Range) && m.covers(tooth_id));
        if let Some(check) = &seg.check
            && by_number
            && check.primary_only.iter().any(|n| n == tooth_id)
            && !check.second_only.is_empty()
        {
            report.disputed.push(DisputedNumber {
                path: path.to_string(),
                number: tooth_id.to_string(),
                heard_instead: check.second_only.clone(),
                cited: cited.clone(),
            });
        }
    }

    // What was said about this tooth: only the clauses that name it count.
    let v = view(&item);
    let def = chart().finding(v.finding);
    let windows: Vec<Vec<(usize, usize)>> = lines.iter().map(|l| l.windows(tooth_id)).collect();
    if let Some(f) = def
        && windows.iter().any(|w| !w.is_empty())
    {
        let absent_negates = f.id != "missing";
        let inside: Vec<Words> =
            lines.iter().zip(&windows).flat_map(|(l, w)| l.words(w, absent_negates)).collect();
        let outside: Vec<Words> =
            lines.iter().zip(&windows).flat_map(|(l, w)| l.words(&l.outside(w), absent_negates)).collect();
        // The rest of this tooth's sentences, for a finding said once for
        // two teeth: "309 has grade one mobility and 409 is grade three",
        // "309 is fractured and 409 too".
        let beside: Vec<Words> =
            lines.iter().zip(&windows).flat_map(|(l, w)| l.words(&l.beside(w), absent_negates)).collect();
        let near: HashSet<String> = lines.iter().zip(&windows).flat_map(|(l, w)| l.numbers(w)).collect();
        let far: HashSet<String> =
            lines.iter().zip(&windows).flat_map(|(l, w)| l.numbers(&l.outside(w))).collect();
        let values: Vec<(&str, String)> = [("grade", v.grade), ("mm", v.mm)]
            .into_iter()
            .filter_map(|(key, n)| n.map(|n| (key, crate::numbers::canonical(&fmt_num(n)))))
            .collect();
        let shared = (!values.is_empty() && values.iter().all(|(_, n)| near.contains(n)))
            || inside
                .iter()
                .any(|w| w.stems.iter().any(|s| matches!(s.as_str(), "too" | "also" | "likewise" | "same")));
        // The finding's own words.
        let mut cue_flagged = false;
        let mut check_cues = |report: &mut ToothReport, cues: &[String]| {
            if cues.is_empty() || cue_flagged {
                return;
            }
            let kind = match any_said(&inside, cues) {
                Said::Yes => return,
                Said::Negated => ToothFlagKind::Negated,
                Said::No if shared && any_said(&beside, cues) == Said::Yes => return,
                Said::No if any_said(&outside, cues) == Said::Yes => ToothFlagKind::OtherTooth,
                Said::No => ToothFlagKind::CueMissing,
            };
            cue_flagged = true;
            report.flag(path, tooth_id, kind, None, cues.to_vec());
        };
        check_cues(&mut report, &f.cues);
        let detail = v.detail.and_then(|d| f.details.iter().find(|x| x.id == d));
        if let Some(d) = detail {
            for group in &d.cues {
                check_cues(&mut report, group);
            }
            // Words that rule this type out: "uncomplicated", "no pulp".
            if let Some(x) = d.exclude.iter().find(|x| said_in(&inside, x) == Said::Yes) {
                report.flag(path, tooth_id, ToothFlagKind::Negated, None, vec![x.clone()]);
            }
            // Words for another type of a finding that has one type per tooth.
            if f.one_per_tooth {
                let mine: Vec<&Vec<String>> = d.cues.iter().collect();
                let other = f.details.iter().filter(|o| o.id != d.id && !o.cues.is_empty()).find(|o| {
                    o.cues.iter().all(|g| any_said(&inside, g) == Said::Yes)
                        && !o.cues.iter().all(|g| mine.contains(&g))
                });
                if let Some(o) = other {
                    report.flag(path, tooth_id, ToothFlagKind::OtherType, None, vec![o.words.clone()]);
                }
            }
        }
        // The grade and the millimetres: said in this tooth's clause, or only
        // in another tooth's.
        let elsewhere: Vec<String> = values
            .iter()
            .filter(|(_, n)| !near.contains(n) && far.contains(n))
            .map(|(key, n)| format!("{key} {n}"))
            .collect();
        if !elsewhere.is_empty() {
            report.flag(path, tooth_id, ToothFlagKind::OtherTooth, None, elsewhere);
        }
        // Charted as done, but said as a plan.
        if f.plannable && !v.planned {
            let all: Vec<bool> = lines.iter().zip(&windows).map(|(l, w)| sounds_planned(l, w)).collect();
            if all.iter().any(|p| *p) {
                report.flag(path, tooth_id, ToothFlagKind::Planned, None, Vec::new());
            }
        }
    }

    if let Err(misfit) = fits(&item) {
        report.flag(path, tooth_id, ToothFlagKind::Shape, None, vec![misfit.to_string()]);
    }
    report
}

/// Findings that can't sit beside each other on one tooth: `normal` beside
/// anything, `missing` beside anything not planned, and two different items
/// of a finding with one per tooth (two resorption stages, two extraction
/// techniques). Exact repeats are [`ToothFlagKind::Duplicate`] instead.
fn conflicts(items: &[Value], path: &str) -> Vec<ToothFlag> {
    let mut out: Vec<ToothFlag> = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let a = view(item);
        let tooth = item["tooth"].as_str().unwrap_or_default();
        for (m, earlier) in items.iter().enumerate().take(n) {
            if earlier["tooth"].as_str() != Some(tooth) || earlier == item {
                continue;
            }
            let b = view(earlier);
            let same_key =
                ["finding", "detail", "grade", "mm", "planned"].iter().all(|k| item[*k] == earlier[*k]);
            if same_key {
                continue; // a duplicate
            }
            let one = chart().finding(a.finding).is_some_and(|f| f.one_per_tooth);
            let clash = (a.finding == "normal") != (b.finding == "normal")
                || (a.finding == "missing" && b.finding != "missing" && !b.planned)
                || (b.finding == "missing" && a.finding != "missing" && !a.planned)
                || (a.finding == b.finding && one);
            if clash {
                out.push(ToothFlag {
                    path: format!("{path}/{n}"),
                    tooth: tooth.to_string(),
                    kind: ToothFlagKind::Conflict,
                    other: Some(format!("{path}/{m}")),
                    expected: vec![code_of(earlier).english()],
                });
                break;
            }
        }
    }
    out
}

/// Items repeating an earlier item's tooth, finding, detail, grade, mm and
/// planned, then findings on one tooth that contradict each other.
pub fn duplicates(items: &[Value], path: &str) -> Vec<ToothFlag> {
    let key = |i: &Value| {
        ["tooth", "finding", "detail", "grade", "mm", "planned"].map(|k| i[k].to_string()).join("|")
    };
    let mut seen = HashSet::new();
    let mut out: Vec<ToothFlag> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| !seen.insert(key(item)))
        .map(|(n, item)| ToothFlag {
            path: format!("{path}/{n}"),
            tooth: item["tooth"].as_str().unwrap_or_default().to_string(),
            kind: ToothFlagKind::Duplicate,
            other: None,
            expected: Vec::new(),
        })
        .collect();
    out.extend(conflicts(items, path));
    out
}

/// Every check on a whole chart heading (`path` is the heading's pointer,
/// "/dental_chart"): each item, then duplicates and conflicts.
pub fn check_chart(
    value: &Value,
    path: &str,
    transcript: &Transcript,
    dentition: Option<Dentition>,
) -> ToothReport {
    let mut report = ToothReport::default();
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    for (n, item) in items.iter().enumerate() {
        if let Value::Object(map) = item
            && is_tooth_item(map)
        {
            report.merge(check_item(map, &format!("{path}/{n}"), transcript, dentition));
        }
    }
    report.flags.extend(duplicates(items, path));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn permanent(d: Dentition) -> Vec<&'static str> {
        chart()
            .teeth
            .iter()
            .filter(|t| t.dentitions.contains(&d) && !t.deciduous)
            .map(|t| t.id.as_str())
            .collect()
    }

    fn deciduous(d: Dentition) -> Vec<&'static str> {
        chart()
            .teeth
            .iter()
            .filter(|t| t.dentitions.contains(&d) && t.deciduous)
            .map(|t| t.id.as_str())
            .collect()
    }

    // --- Step 1: the shared data, against the AVDC table ---

    #[test]
    fn dentitions_match_the_avdc_counts() {
        assert_eq!(permanent(Dentition::Canine).len(), 42);
        assert_eq!(deciduous(Dentition::Canine).len(), 28);
        assert_eq!(permanent(Dentition::Feline).len(), 30);
        assert_eq!(deciduous(Dentition::Feline).len(), 26);
        assert_eq!(ids(Some(Dentition::Canine)).len(), 70);
        assert_eq!(ids(Some(Dentition::Feline)).len(), 56);
        assert_eq!(ids(None), ids(Some(Dentition::Canine)), "no dentition: the dog set");
    }

    #[test]
    fn every_cat_tooth_is_a_dog_tooth() {
        let dog: HashSet<&str> = ids(Some(Dentition::Canine)).into_iter().collect();
        for id in ids(Some(Dentition::Feline)) {
            assert!(dog.contains(id), "{id}");
        }
    }

    #[test]
    fn triadan_rules_hold() {
        for t in &chart().teeth {
            let pos = t.position();
            if pos == 4 {
                assert_eq!(t.kind, ToothType::Canine, "{}", t.id);
            }
            if pos == 9 {
                assert!(!t.deciduous && t.kind == ToothType::Molar && t.ordinal == 1, "{}", t.id);
            }
            let q: u8 = t.id[..1].parse().unwrap();
            assert_eq!(t.deciduous, q >= 5, "{}", t.id);
            assert_eq!(t.jaw == Jaw::Upper, matches!(q, 1 | 2 | 5 | 6), "{}", t.id);
            assert_eq!(t.side == Side::Right, matches!(q, 1 | 4 | 5 | 8), "{}", t.id);
        }
        let carnassials: Vec<&str> =
            chart().teeth.iter().filter(|t| t.shape == Shape::Carnassial).map(|t| t.id.as_str()).collect();
        assert_eq!(carnassials, ["108", "208", "309", "409"]);
        let cat = ids(Some(Dentition::Feline));
        for absent in ["105", "205", "305", "306", "405", "406", "706", "806"] {
            assert!(!cat.contains(&absent), "a cat has no {absent}");
        }
        for t in chart().teeth.iter().filter(|t| t.deciduous) {
            assert_ne!(t.position(), 5, "no x05 in a deciduous set: {}", t.id);
        }
        assert!(tooth("505").is_none() && tooth("111").is_none() && tooth("312").is_none());
    }

    #[test]
    fn each_row_lists_its_teeth_once() {
        for d in [Dentition::Canine, Dentition::Feline] {
            let rows = chart().rows(d);
            for (name, row) in rows.top_to_bottom() {
                let expected: Vec<&str> = chart()
                    .teeth
                    .iter()
                    .filter(|t| t.dentitions.contains(&d))
                    .filter(|t| {
                        let jaw = if t.jaw == Jaw::Upper { "upper" } else { "lower" };
                        let key = if t.deciduous { format!("{jaw}_deciduous") } else { jaw.to_string() };
                        key == name
                    })
                    .map(|t| t.id.as_str())
                    .collect();
                let mut listed: Vec<&str> = row.iter().map(String::as_str).collect();
                let unique: HashSet<&str> = listed.iter().copied().collect();
                assert_eq!(unique.len(), listed.len(), "{d:?} {name}: listed once");
                // Drawing order: patient's right first, outermost first; then the left from the midline.
                let half = listed.len() / 2;
                assert!(listed[..half].iter().all(|t| tooth(t).unwrap().side == Side::Right), "{d:?} {name}");
                assert!(listed[half..].iter().all(|t| tooth(t).unwrap().side == Side::Left), "{d:?} {name}");
                assert!(
                    listed[half - 1].ends_with("01") && listed[half].ends_with("01"),
                    "{d:?} {name}: midline"
                );
                listed.sort_unstable();
                assert_eq!(listed, expected, "{d:?} {name}");
            }
        }
        assert_eq!(chart().rows(Dentition::Canine).lower.len(), 22);
    }

    #[test]
    fn every_shape_parses_inside_its_box() {
        for shape in [Shape::Incisor, Shape::Canine, Shape::Premolar, Shape::Carnassial, Shape::Molar] {
            let cmds = parse_path(chart().shapes.get(shape)).unwrap_or_else(|e| panic!("{shape:?}: {e}"));
            assert!(matches!(cmds.last(), Some(PathCmd::Close)), "{shape:?} is closed");
            let [w, h] = chart().geometry.tooth;
            let inside = |x: f32, y: f32| (0.0..=w).contains(&x) && (0.0..=h).contains(&y);
            for c in cmds {
                match c {
                    PathCmd::Move(x, y) | PathCmd::Line(x, y) => assert!(inside(x, y), "{shape:?}"),
                    PathCmd::Curve(a, b, c, d, e, f) => {
                        assert!(inside(a, b) && inside(c, d) && inside(e, f), "{shape:?}")
                    }
                    PathCmd::Close => {}
                }
            }
        }
        assert!(parse_path("M 1 2 Q 3 4 5 6").is_err());
        assert!(parse_path("L 1 2").is_err());
        assert!(parse_path("M 1").is_err());
    }

    #[test]
    fn every_detail_has_a_code_and_pd_never_stands_alone() {
        for f in &chart().findings {
            for d in &f.details {
                assert!(!d.code.is_empty(), "{}/{}", f.id, d.id);
                if d.with_grade.is_none() {
                    assert!(
                        f.code_detail.is_some(),
                        "{}/{}: a detail with a code needs a pattern",
                        f.id,
                        d.id
                    );
                }
            }
            for code in [&f.code, &f.code_grade, &f.code_detail].into_iter().flatten() {
                assert_ne!(code, "PD", "{}: PD means periodontal disease stage, with its number", f.id);
            }
            if f.code_grade.is_some() {
                assert!(f.grade.is_some() && f.words_grade.is_some(), "{}", f.id);
            }
            assert!(f.mm == MmUse::None || f.words_mm.is_some() || f.text_required, "{}", f.id);
            assert!(f.text_required || !f.cues.is_empty(), "{}: cue words", f.id);
        }
        assert_eq!(chart().findings.len(), 20, "the finding enum of ADR-0021 decision 2");
    }

    // --- Step 3: names, the reader and the code table ---

    fn read(text: &str) -> Vec<String> {
        teeth_in(text).into_iter().flat_map(|m| m.teeth).collect()
    }

    #[test]
    fn every_tooth_name_reads_back_as_its_number() {
        for t in &chart().teeth {
            let name = name_of(&t.id).unwrap().english();
            assert_eq!(read(&name), vec![t.id.clone()], "{name}");
            let m = &teeth_in(&name)[0];
            assert_eq!(m.said_as, SaidAs::Name);
        }
        assert_eq!(name_of("108").unwrap().english(), "right maxillary fourth premolar");
        assert_eq!(name_of("704").unwrap().english(), "left mandibular deciduous canine");
        assert_eq!(name_of("409").unwrap().english(), "right mandibular first molar");
        assert!(name_of("105x").is_none());
    }

    #[test]
    fn spoken_numbers_and_names_read_as_teeth() {
        let cases: &[(&str, &[&str])] = &[
            ("one oh eight", &["108"]),
            ("One o eight has a slab fracture", &["108"]),
            ("one zero eight", &["108"]),
            ("1 08", &["108"]),
            ("108", &["108"]),
            ("two-oh-eight", &["208"]),
            ("one hundred and eight", &["108"]),
            ("upper right fourth premolar", &["108"]),
            ("right maxillary P4", &["108"]),
            ("the right maxillary P four", &["108"]),
            ("the right upper carnassial", &["108"]),
            ("lower left first molar", &["309"]),
            ("left mandibular carnassial", &["309"]),
            ("the lower right canine", &["404"]),
            ("baby upper right canine", &["504"]),
            ("deciduous right maxillary canine", &["504"]),
            ("The baby lower left canine is still there too.", &["704"]),
            ("three ten", &["310"]),
            ("four eleven", &["411"]),
            ("the upper left central incisor", &["201"]),
            ("lower right corner incisor", &["403"]),
            ("the upper left fourth pre-molar", &["208"]),
            ("the upper left canine tooth", &["204"]),
        ];
        for (text, expected) in cases {
            assert_eq!(read(text), *expected, "{text}");
        }
        assert_eq!(teeth_in("upper right fourth premolar")[0].said_as, SaidAs::Name);
        assert_eq!(teeth_in("one oh eight")[0].said_as, SaidAs::Number);
        assert_eq!(teeth_in("Four oh four, the lower right canine, looks fine.").len(), 2);
    }

    #[test]
    fn ranges_expand_along_one_arcade_only() {
        assert_eq!(read("101 through 103"), ["101", "102", "103"]);
        assert_eq!(teeth_in("101 through 103")[0].said_as, SaidAs::Range);
        assert_eq!(read("One oh one through one oh three are missing."), ["101", "102", "103"]);
        assert_eq!(read("one oh three through two oh three"), ["103", "102", "101", "201", "202", "203"]);
        // "To" and a hyphen make a range only inside one quadrant: across
        // the midline they are as often a movement as a range.
        assert_eq!(read("one oh three to two oh three"), ["103", "203"]);
        assert_eq!(
            read("Moving from one oh eight to two oh eight, which has a slab fracture."),
            ["108", "208"]
        );
        assert_eq!(read("One oh eight - two oh eight are both fractured."), ["108", "208"]);
        assert_eq!(read("one oh six to one oh eight"), ["106", "107", "108"]);
        assert_eq!(read("teeth 306-308"), ["306", "307", "308"]);
        // Across jaws: both teeth were said, nothing between them.
        let across = teeth_in("108 to 308");
        assert_eq!(across.len(), 2);
        assert!(across.iter().all(|m| m.said_as == SaidAs::Number));
        assert_eq!(read("108 to 308"), ["108", "308"]);
        assert_eq!(read("104 to 504"), ["104", "504"], "permanent to deciduous isn't a range");
        assert_eq!(read("Two oh eight to be extracted"), ["208"]);
    }

    #[test]
    fn groups_expand_to_their_teeth() {
        assert_eq!(
            read("All the lower incisors are mobile, grade three."),
            ["301", "302", "303", "401", "402", "403"]
        );
        assert_eq!(teeth_in("all the lower incisors")[0].said_as, SaidAs::Group);
        assert_eq!(read("upper canines"), ["104", "204"]);
        assert_eq!(
            read("The adult upper canines, one oh four and two oh four"),
            ["104", "204", "104", "204"]
        );
        assert_eq!(read("persistent deciduous upper canines"), ["504", "604"]);
        assert_eq!(read("the upper right incisors"), ["101", "102", "103"]);
        assert_eq!(read("all the canines"), ["104", "204", "304", "404"]);
        assert_eq!(read("all four canines"), ["104", "204", "304", "404"]);
        assert_eq!(read("the four canines"), ["104", "204", "304", "404"]);
        assert_eq!(read("lower premolars").len(), 8);
        assert_eq!(read("the upper fourth premolars"), ["108", "208"]);
        assert_eq!(read("both left carnassials"), ["208", "309"]);
    }

    #[test]
    fn ambiguous_phrases_give_no_tooth() {
        for text in [
            "the canine",
            "a premolar",
            "the left side",
            "the upper canine",
            "the right first molar",
            "a canine patient",
            "stage three periodontal disease",
            "Left caudal mandibular block, point two mils of bupivacaine.",
            "everything else looks normal",
            "the right upper and lower canine",
        ] {
            assert!(teeth_in(text).is_empty(), "{text}: {:?}", teeth_in(text));
        }
    }

    #[test]
    fn doses_times_and_vitals_are_not_teeth() {
        for text in [
            "three oh nine pm",
            "three oh nine p.m.",
            "a 108 mg tablet",
            "one oh eight milligrams",
            "heart rate one hundred and eight",
            "weighs 104 pounds",
            "we'll see her at three ten",
            "108.5",
            "1,080 dollars",
            "a temperature of 101",
        ] {
            assert!(teeth_in(text).is_empty(), "{text}: {:?}", teeth_in(text));
        }
        assert_eq!(read("Look at one oh eight again"), ["108"]);
        assert_eq!(read("nine oh nine"), Vec::<String>::new(), "not a Triadan id");
        assert_eq!(read("five oh five"), Vec::<String>::new(), "no deciduous first premolar");
    }

    fn item(
        tooth: &str,
        finding: &str,
        detail: Option<&str>,
        grade: Option<u8>,
        mm: Option<f64>,
        planned: bool,
    ) -> Value {
        json!({"tooth": tooth, "text": "as said", "finding": finding, "detail": detail, "grade": grade, "mm": mm, "planned": planned, "evidence": [0]})
    }

    #[test]
    fn codes_come_from_the_table() {
        let c = |i: Value| code_of(&i);
        let p = c(item("309", "periodontal_stage", None, Some(3), None, false));
        assert_eq!(p.code.as_deref(), Some("PD3"));
        assert_eq!(p.english(), "Periodontal disease stage 3 (PD3)");
        assert_eq!(c(item("309", "mobility", None, Some(2), None, false)).english(), "Mobility 2 (M2)");
        assert_eq!(c(item("309", "furcation", None, Some(2), None, false)).english(), "Furcation 2 (F2)");
        assert_eq!(
            c(item("108", "fracture", Some("complicated_crown"), None, None, false)).english(),
            "Complicated crown fracture (T/FX/CCF)"
        );
        assert_eq!(c(item("304", "fracture", None, None, None, false)).english(), "Fracture (T/FX)");
        let tr = c(item("307", "resorption", Some("crown_worse"), Some(4), None, false));
        assert_eq!(tr.code.as_deref(), Some("TR4b"));
        assert_eq!(tr.english(), "Tooth resorption stage 4b (TR4b)");
        assert_eq!(c(item("307", "resorption", None, None, None, false)).english(), "Tooth resorption (TR)");
        assert_eq!(
            c(item("307", "resorption_type", None, Some(2), None, false)).english(),
            "Resorption type 2 (T2)"
        );
        assert_eq!(
            c(item("108", "extraction", Some("open"), None, None, false)).english(),
            "Open extraction (XSS)"
        );
        assert_eq!(
            c(item("309", "extraction", Some("closed"), None, None, false)).english(),
            "Closed extraction (X)"
        );
        assert_eq!(
            c(item("304", "extraction", Some("closed_sectioned"), None, None, true)).english(),
            "Planned: closed extraction, sectioned (XS)"
        );
        let planned = c(item("304", "extraction", None, None, None, true));
        assert_eq!(planned.english(), "Planned: extraction");
        assert_eq!(planned.code, None);
        let depth = c(item("309", "probing_depth", None, None, Some(7.0), false));
        assert_eq!(depth.english(), "Probing depth 7 mm");
        assert_eq!(depth.code, None, "pocket depth is never PD");
        assert_eq!(
            c(item("109", "gingival_recession", None, None, Some(2.5), false)).english(),
            "Gingival recession 2.5 mm (GR)"
        );
        assert_eq!(c(item("209", "missing", None, None, None, false)).english(), "Missing");
        assert_eq!(c(item("404", "normal", None, None, None, false)).english(), "Normal");
        assert_eq!(
            c(item("504", "persistent_deciduous", None, None, None, false)).english(),
            "Persistent deciduous tooth (DT/P)"
        );
        assert_eq!(c(item("304", "unerupted", None, None, None, false)).english(), "Unerupted (T/U)");
        assert_eq!(c(item("204", "wear", Some("attrition"), None, None, false)).english(), "Attrition (AT)");
        assert_eq!(c(item("204", "wear", None, None, None, false)).english(), "Wear");
        assert_eq!(
            c(item("304", "malposition", Some("linguoversion"), None, None, false)).english(),
            "Linguoversion (LV)",
            "a tooth version never implies a malocclusion class"
        );
        assert_eq!(
            c(item("307", "crown_amputation", None, None, None, false)).english(),
            "Crown amputation (CR/A)"
        );
        assert_eq!(c(item("104", "discoloured", None, None, None, false)).english(), "Discoloured");
        // Other, and fields that don't fit, print the words as said with no code.
        assert_eq!(c(item("104", "other", None, None, None, false)).english(), "as said");
        let odd = c(item("108", "fracture", Some("open"), None, None, false));
        assert_eq!((odd.english(), odd.code), ("as said".to_string(), None));
    }

    #[test]
    fn printed_words_translate_through_a_table() {
        let p = code_of(&item("108", "extraction", Some("open"), None, None, true));
        let tr = |k: &str| match k {
            "Open extraction" => "Offene Extraktion".to_string(),
            "Planned: {finding}" => "Geplant: {finding}".to_string(),
            other => other.to_string(),
        };
        assert_eq!(p.render(&tr, false), "Geplant: Offene Extraktion (XSS)");
        assert_eq!(p.render(&tr, true), "Geplant: offene Extraktion (XSS)");
    }

    #[test]
    fn fits_refuses_fields_that_dont_fit() {
        let f = |i: Value| fits(&i);
        assert_eq!(
            f(item("309", "mobility", None, Some(4), None, false)).unwrap_err().to_string(),
            "Mobility takes a grade from 0 to 3."
        );
        assert!(matches!(
            f(item("108", "fracture", Some("open"), None, None, false)),
            Err(Misfit::UnknownDetail { .. })
        ));
        assert!(matches!(
            f(item("309", "probing_depth", None, None, None, false)),
            Err(Misfit::MmMissing { .. })
        ));
        assert!(matches!(
            f(item("307", "resorption", Some("crown_worse"), Some(3), None, false)),
            Err(Misfit::DetailNeedsGrade { .. })
        ));
        assert!(matches!(
            f(item("307", "resorption_type", None, None, None, false)),
            Err(Misfit::GradeMissing { .. })
        ));
        assert!(matches!(
            f(item("309", "mobility", None, None, Some(2.0), false)),
            Err(Misfit::MmNotUsed { .. })
        ));
        assert!(matches!(
            f(item("209", "missing", None, Some(1), None, false)),
            Err(Misfit::GradeNotUsed { .. })
        ));
        assert!(matches!(
            f(item("108", "fracture", None, None, None, true)),
            Err(Misfit::NotPlannable { .. })
        ));
        assert!(matches!(f(item("108", "sparkly", None, None, None, false)), Err(Misfit::UnknownFinding(_))));
        assert!(matches!(
            fits(
                &json!({"tooth": "104", "text": " ", "finding": "other", "detail": null, "grade": null, "mm": null, "planned": false})
            ),
            Err(Misfit::TextMissing)
        ));
        for ok in [
            item("309", "mobility", None, Some(3), None, false),
            item("309", "mobility", None, None, None, false),
            item("307", "resorption", Some("root_worse"), Some(4), None, false),
            item("304", "extraction", None, None, None, true),
            item("109", "gingival_recession", None, None, None, false),
        ] {
            assert_eq!(fits(&ok), Ok(()), "{ok}");
        }
        assert_eq!(
            tooth_misfit("105", Some(Dentition::Feline)).unwrap().to_string(),
            "105 isn't a cat's tooth."
        );
        assert_eq!(tooth_misfit("105", Some(Dentition::Canine)), None);
        assert_eq!(tooth_misfit("105", None), None);
        assert_eq!(tooth_misfit("999", None).unwrap().to_string(), "\"999\" isn't a tooth number.");
    }

    #[test]
    fn legend_lists_only_the_codes_used() {
        let items = vec![
            item("108", "fracture", Some("complicated_crown"), None, None, false),
            item("108", "extraction", Some("open"), None, None, false),
            item("309", "mobility", None, Some(2), None, false),
            item("310", "mobility", None, Some(1), None, false),
            item("209", "missing", None, None, None, false),
            item("109", "gingival_recession", None, None, Some(2.0), false),
            item("309", "probing_depth", None, None, Some(7.0), false),
            item("309", "periodontal_stage", None, None, None, false),
        ];
        assert_eq!(
            legend(&items),
            vec![
                LegendLine::Code { code: "T/FX/CCF".into(), words: "Complicated crown fracture".into() },
                LegendLine::Index("M0-M3: tooth mobility index (AVDC). Grades as said."),
                LegendLine::Index("Probing depth is in millimetres."),
                LegendLine::Code { code: "GR".into(), words: "Gingival recession".into() },
                LegendLine::Code { code: "XSS".into(), words: "Open extraction".into() },
            ],
            "no PD line: no stage was said"
        );
        let items = [
            item("309", "missing", None, None, None, false),
            item("108", "normal", None, None, None, false),
            item("309", "caries", None, None, None, false),
        ];
        let grouped = by_tooth(&items);
        assert_eq!(
            grouped.iter().map(|(t, l)| (t.as_str(), l.len())).collect::<Vec<_>>(),
            [("108", 1), ("309", 2)]
        );
    }

    // --- Step 4: faithfulness for teeth ---

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

    fn charted(
        tooth: &str,
        finding: &str,
        detail: Option<&str>,
        grade: Option<u8>,
        evidence: &[u32],
    ) -> Value {
        json!({"tooth": tooth, "text": "as said", "finding": finding, "detail": detail, "grade": grade, "mm": null, "planned": false, "evidence": evidence})
    }

    fn check(item: &Value, t: &Transcript) -> ToothReport {
        check_item(item.as_object().unwrap(), "/dental_chart/0", t, Some(Dentition::Canine))
    }

    #[test]
    fn a_tooth_said_by_number_and_name_passes() {
        let t = transcript(&["Four oh four, the lower right canine, looks fine."]);
        let r = check(&charted("404", "normal", None, None, &[0]), &t);
        assert_eq!(r, ToothReport { checked: 1, ..ToothReport::default() });
    }

    #[test]
    fn a_tooth_said_only_by_name_passes() {
        let t = transcript(&["The upper right canine is normal."]);
        let r = check(&charted("104", "normal", None, None, &[0]), &t);
        assert!(r.unsupported.is_empty() && r.mismatches.is_empty() && r.flags.is_empty(), "{r:?}");
    }

    #[test]
    fn ranges_and_groups_pass_for_every_tooth_in_them() {
        let t = transcript(&[
            "Charting now. One oh one through one oh three are missing.",
            "All the lower incisors are mobile, grade three.",
        ]);
        for id in ["101", "102", "103"] {
            let r = check(&charted(id, "missing", None, None, &[0]), &t);
            assert!(r.unsupported.is_empty() && r.mismatches.is_empty() && r.flags.is_empty(), "{id}: {r:?}");
        }
        let items: Vec<Value> = ["301", "302", "303", "401", "402", "403"]
            .iter()
            .map(|id| charted(id, "mobility", None, Some(3), &[1]))
            .collect();
        let r = check_chart(&Value::Array(items), "/dental_chart", &t, Some(Dentition::Canine));
        assert_eq!(r.checked, 6);
        assert!(r.unsupported.is_empty() && r.mismatches.is_empty() && r.flags.is_empty(), "{r:?}");
        // 104 isn't in the range or the group.
        assert_eq!(check(&charted("104", "missing", None, None, &[0]), &t).unsupported.len(), 1);
    }

    #[test]
    fn a_tooth_said_nowhere_is_a_hard_fail() {
        let t = transcript(&["One oh four has a fracture."]);
        let r = check(&charted("105", "fracture", None, None, &[0]), &t);
        assert_eq!(r.unsupported.len(), 1);
        assert_eq!(r.unsupported[0].number, "105");
        assert!(r.unsupported[0].found_in.is_empty());
        assert!(r.mismatches.is_empty());
    }

    #[test]
    fn a_tooth_said_on_another_line_is_a_citation_mismatch() {
        let t = transcript(&[
            "Charting now.",
            "Two oh eight is fine.",
            "Some calculus.",
            "Scaling done.",
            "",
            "Three oh nine is mobile.",
        ]);
        let r = check(&charted("309", "mobility", None, None, &[2]), &t);
        assert!(r.unsupported.is_empty());
        assert_eq!(r.mismatches.len(), 1);
        assert_eq!(r.mismatches[0].found_in, vec![5]);
        assert_eq!(r.mismatches[0].cited, vec![2]);
    }

    #[test]
    fn a_number_and_a_name_that_disagree_are_flagged_not_corrected() {
        let t = transcript(&[
            "Alright, charting. One oh eight, upper left fourth premolar, has a slab fracture with pulp exposure. That one needs to come out.",
        ]);
        let r = check(&charted("108", "fracture", Some("complicated_crown"), None, &[0]), &t);
        assert!(r.unsupported.is_empty() && r.mismatches.is_empty(), "108 was said: {r:?}");
        assert_eq!(r.flags.len(), 1, "{r:?}");
        assert_eq!(r.flags[0].kind, ToothFlagKind::NameConflict);
        assert_eq!(r.flags[0].other.as_deref(), Some("208"));
        // When the name's own number was said too, there is no conflict.
        let t = transcript(&[
            "One oh eight has a fracture, and two oh eight, the upper left fourth premolar, is fine.",
        ]);
        assert!(check(&charted("108", "fracture", None, None, &[0]), &t).flags.is_empty());
        // A matching name is no conflict.
        let t = transcript(&["One oh eight, upper right fourth premolar, has a slab fracture."]);
        assert!(check(&charted("108", "fracture", None, None, &[0]), &t).flags.is_empty());
        // "Which is the …" restates the number: a conflict when it disagrees.
        let t = transcript(&["One oh eight, which is the upper left fourth premolar, is fractured."]);
        assert_eq!(
            check(&charted("108", "fracture", None, None, &[0]), &t).flags[0].other.as_deref(),
            Some("208")
        );
        // Another tooth in another clause is no conflict.
        let t = transcript(&["Two oh six is missing, and the upper left canine looks healthy."]);
        assert!(check(&charted("206", "missing", None, None, &[0]), &t).flags.is_empty());
        assert!(check(&charted("204", "normal", None, None, &[0]), &t).flags.is_empty());
    }

    /// The scripted dental consults (packages/eval/fixtures/scripts): every
    /// expected chart fact, cited to the lines that name its tooth, passes the
    /// tooth checks with no flag, so a correct draft is a clean draft. The old
    /// `dental` fixture keeps its wrong-side name, which is flagged.
    #[test]
    fn the_dental_fixtures_chart_cleanly_from_their_own_lines() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/eval/fixtures/scripts");
        let load = |name: &str| -> Value {
            let text = std::fs::read_to_string(dir.join(format!("{name}.yaml"))).unwrap();
            serde_yaml_ng::from_str(&text).unwrap()
        };
        let lines_of = |script: &Value| -> Transcript {
            let lines: Vec<String> = script["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l[1].as_str().unwrap().to_string())
                .collect();
            transcript(&lines.iter().map(String::as_str).collect::<Vec<_>>())
        };
        for (name, facts) in [("dental-chart", 26), ("dental-feline", 9), ("dental-young-dog", 9)] {
            let script = load(name);
            let t = lines_of(&script);
            let dentition = Dentition::from_species_key(script["species"].as_str().unwrap());
            let mut items = Vec::new();
            for fact in script["expected"]["chart"].as_array().unwrap() {
                let id = fact["tooth"].as_str().unwrap();
                assert!(has_tooth(dentition, id), "{name}: {id} is in the mouth");
                let evidence: Vec<u32> = t
                    .segments
                    .iter()
                    .filter(|s| teeth_in(&s.text).iter().any(|m| m.covers(id)))
                    .map(|s| s.id)
                    .collect();
                assert!(!evidence.is_empty(), "{name}: {id} is said");
                let field = |k: &str| fact.get(k).cloned().unwrap_or(Value::Null);
                items.push(json!({
                    "tooth": id, "text": "as said", "finding": fact["finding"], "detail": field("detail"),
                    "grade": field("grade"), "mm": field("mm"), "planned": fact["planned"].as_bool().unwrap_or(false),
                    "evidence": evidence,
                }));
                if let Some(words) = fact["cites"].as_str() {
                    assert!(t.segments.iter().any(|s| s.text.contains(words)), "{name}: a line says {words}");
                }
            }
            assert_eq!(items.len(), facts, "{name}");
            for item in &items {
                assert_eq!(fits(item), Ok(()), "{name}: {item}");
            }
            let r = check_chart(&Value::Array(items), "/dental_chart", &t, dentition);
            assert_eq!(r.checked as usize, facts);
            assert!(
                r.unsupported.is_empty() && r.mismatches.is_empty() && r.flags.is_empty(),
                "{name}: {r:#?}"
            );
            // Whole-mouth lines name no tooth.
            for s in &t.segments {
                if s.text.contains("across the mouth") || s.text.contains("No other resorptive") {
                    assert!(teeth_in(&s.text).is_empty(), "{name}: {}", s.text);
                }
            }
        }
        let old = load("dental");
        let t = lines_of(&old);
        let line = t.segments.iter().find(|s| s.text.contains("upper left fourth premolar")).unwrap().id;
        let r = check(&charted("108", "fracture", Some("complicated_crown"), None, &[line]), &t);
        assert_eq!(
            r.flags.iter().map(|f| (f.kind, f.other.as_deref())).collect::<Vec<_>>(),
            [(ToothFlagKind::NameConflict, Some("208"))]
        );
    }

    #[test]
    fn missing_cue_words_are_flagged() {
        let t = transcript(&[
            "Two oh nine is missing.",
            "Three oh nine is fractured.",
            "Four oh eight is extracted surgically.",
        ]);
        let r = check(&charted("209", "normal", None, None, &[0]), &t);
        assert_eq!(r.flags.len(), 1);
        assert_eq!(r.flags[0].kind, ToothFlagKind::CueMissing);
        assert!(r.flags[0].expected.contains(&"normal".to_string()));
        assert!(
            check(&charted("309", "fracture", None, None, &[1]), &t).flags.is_empty(),
            "no type said, none charted"
        );
        let r = check(&charted("309", "fracture", Some("complicated_crown"), None, &[1]), &t);
        assert_eq!(r.flags[0].kind, ToothFlagKind::CueMissing, "a guessed type: no pulp word");
        assert!(r.flags[0].expected.contains(&"pulp".to_string()));
        assert!(check(&charted("408", "extraction", Some("open"), None, &[2]), &t).flags.is_empty());
        assert_eq!(
            check(&charted("408", "extraction", Some("closed"), None, &[2]), &t).flags[0].kind,
            ToothFlagKind::CueMissing
        );
    }

    #[test]
    fn shape_and_duplicate_flags() {
        let t = transcript(&["Three oh nine has grade two mobility."]);
        let r = check(&charted("309", "mobility", Some("open"), Some(2), &[0]), &t);
        assert_eq!(r.flags.iter().map(|f| f.kind).collect::<Vec<_>>(), [ToothFlagKind::Shape]);
        let items = vec![
            charted("309", "mobility", None, Some(2), &[0]),
            charted("309", "mobility", None, Some(2), &[0]),
        ];
        let r = check_chart(&Value::Array(items), "/dental_chart", &t, Some(Dentition::Canine));
        assert_eq!(r.flags.len(), 1);
        assert_eq!(
            (r.flags[0].kind, r.flags[0].path.as_str()),
            (ToothFlagKind::Duplicate, "/dental_chart/1")
        );
    }

    #[test]
    fn vet_entered_items_are_not_checked() {
        let t = transcript(&["Nothing about teeth here."]);
        let mut i = charted("105", "missing", None, None, &[]);
        i["entered_by"] = json!("Dr Okafor");
        assert_eq!(check(&i, &t), ToothReport::default());
        let r = check_chart(&Value::Array(vec![i]), "/dental_chart", &t, Some(Dentition::Canine));
        assert_eq!(r.checked, 0);
    }

    #[test]
    fn a_tooth_number_the_second_recogniser_heard_differently_is_disputed() {
        let mut t = transcript(&["One oh eight is fractured."]);
        t.segments[0].check =
            Some(crate::crosscheck::compare("One oh eight is fractured.", "One oh nine is fractured."));
        let r = check(&charted("108", "fracture", None, None, &[0]), &t);
        assert_eq!(r.disputed.len(), 1);
        assert_eq!(r.disputed[0].heard_instead, vec!["109"]);
    }

    #[test]
    fn flags_serialise_compactly_and_round_trip() {
        let flag = ToothFlag {
            path: "/dental_chart/0".into(),
            tooth: "108".into(),
            kind: ToothFlagKind::NameConflict,
            other: Some("208".into()),
            expected: Vec::new(),
        };
        let json = serde_json::to_string(&flag).unwrap();
        assert_eq!(json, r#"{"path":"/dental_chart/0","tooth":"108","kind":"name_conflict","other":"208"}"#);
        assert_eq!(serde_json::from_str::<ToothFlag>(&json).unwrap(), flag);
    }

    // --- Review fixes: what was said about which tooth ---

    fn kinds(r: &ToothReport) -> Vec<ToothFlagKind> {
        r.flags.iter().map(|f| f.kind).collect()
    }

    fn with_mm(tooth: &str, mm: f64, evidence: &[u32]) -> Value {
        json!({"tooth": tooth, "text": "as said", "finding": "probing_depth", "detail": null, "grade": null,
               "mm": mm, "planned": false, "evidence": evidence})
    }

    #[test]
    fn a_finding_said_about_another_tooth_in_the_line_is_flagged() {
        // Swapped between two teeth of one line.
        let t = transcript(&[
            "Two oh eight has a complicated crown fracture with pulp exposure. One oh eight is missing.",
        ]);
        let r = check(&charted("108", "fracture", Some("complicated_crown"), None, &[0]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::OtherTooth), "{r:?}");
        let r = check(&charted("208", "missing", None, None, &[0]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::OtherTooth), "{r:?}");
        // The right way round is clean.
        assert!(
            check(&charted("208", "fracture", Some("complicated_crown"), None, &[0]), &t).flags.is_empty()
        );
        assert!(check(&charted("108", "missing", None, None, &[0]), &t).flags.is_empty());

        // A grade from the other tooth's clause.
        let t = transcript(&["Three oh nine has grade one mobility and four oh nine is grade three."]);
        let r = check(&charted("309", "mobility", None, Some(3), &[0]), &t);
        assert_eq!(kinds(&r), [ToothFlagKind::OtherTooth], "{r:?}");
        assert_eq!(r.flags[0].expected, ["grade 3"]);
        assert!(check(&charted("309", "mobility", None, Some(1), &[0]), &t).flags.is_empty());
        assert!(check(&charted("409", "mobility", None, Some(3), &[0]), &t).flags.is_empty());

        // Millimetres swapped between two teeth.
        let t = transcript(&[
            "Probing depths: three oh nine is nine millimetres, four oh nine is three millimetres.",
        ]);
        assert_eq!(kinds(&check(&with_mm("309", 3.0, &[0]), &t)), [ToothFlagKind::OtherTooth]);
        assert_eq!(kinds(&check(&with_mm("409", 9.0, &[0]), &t)), [ToothFlagKind::OtherTooth]);
        assert!(check(&with_mm("309", 9.0, &[0]), &t).flags.is_empty());
        assert!(check(&with_mm("409", 3.0, &[0]), &t).flags.is_empty());

        // Citing an extra line that doesn't name the tooth adds nothing.
        let t = transcript(&[
            "Four oh four looks fine.",
            "One oh eight has a slab fracture with pulp exposure, grade two mobility.",
        ]);
        let r = check(&charted("404", "fracture", Some("complicated_crown"), None, &[0, 1]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::OtherTooth), "{r:?}");
        let r = check(&charted("404", "mobility", None, Some(2), &[0, 1]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::OtherTooth), "{r:?}");
    }

    /// The E4B eval draft that charted 108 as a closed extraction: "closed"
    /// meant the flap was sutured, and "simple" was said about 309.
    #[test]
    fn a_closed_flap_is_not_a_closed_extraction() {
        let t = transcript(&[
            "Extracting one oh eight surgically, with a gingival flap, closed with four oh Monocryl. Three oh nine is a simple extraction.",
        ]);
        let r = check(&charted("108", "extraction", Some("closed"), None, &[0]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::OtherTooth), "simple is 309's: {r:?}");
        assert!(kinds(&r).contains(&ToothFlagKind::OtherType), "surgically, flap: {r:?}");
        assert!(check(&charted("108", "extraction", Some("open"), None, &[0]), &t).flags.is_empty());
        assert!(check(&charted("309", "extraction", Some("closed"), None, &[0]), &t).flags.is_empty());
        let r = check(&charted("309", "extraction", Some("open"), None, &[0]), &t);
        assert!(!r.flags.is_empty(), "{r:?}");
    }

    #[test]
    fn clauses_split_at_sentences_and_between_teeth() {
        assert_eq!(
            tooth_clauses("Three oh nine has grade one mobility and four oh nine is grade three.", "409"),
            ["four oh nine is grade three"]
        );
        assert_eq!(
            tooth_clauses("No resorption on three oh seven, but four oh eight is mobile.", "307"),
            ["No resorption on three oh seven"]
        );
        assert_eq!(
            tooth_clauses("Five oh four, six oh four and seven oh four, closed, no sectioning.", "604"),
            ["Five oh four, six oh four and seven oh four, closed, no sectioning"]
        );
        assert_eq!(
            tooth_clauses(
                "Alright, charting. One oh eight, upper left fourth premolar, has a slab fracture.",
                "208"
            ),
            ["One oh eight, upper left fourth premolar, has a slab fracture"]
        );
    }

    #[test]
    fn a_negated_finding_is_flagged() {
        let t = transcript(&[
            "No resorption on three oh seven.",
            "Four oh four is not fractured.",
            "Three oh nine isn't mobile.",
            "Two oh nine is not missing, it's just small.",
            "Two oh four has an uncomplicated crown fracture, no pulp exposure.",
            "Extracting five oh four, closed, no sectioning, no flap needed.",
            "Fracture ruled out on two oh six.",
        ]);
        for (tooth, finding, line) in [
            ("307", "resorption", 0),
            ("404", "fracture", 1),
            ("309", "mobility", 2),
            ("209", "missing", 3),
            ("206", "fracture", 6),
        ] {
            let r = check(&charted(tooth, finding, None, None, &[line]), &t);
            assert_eq!(kinds(&r), [ToothFlagKind::Negated], "{tooth} {finding}: {r:?}");
        }
        let r = check(&charted("204", "fracture", Some("complicated_crown"), None, &[4]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::Negated), "{r:?}");
        assert!(
            check(&charted("204", "fracture", Some("uncomplicated_crown"), None, &[4]), &t).flags.is_empty()
        );
        let r = check(&charted("504", "extraction", Some("closed_sectioned"), None, &[5]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::Negated), "{r:?}");
        let r = check(&charted("504", "extraction", Some("open"), None, &[5]), &t);
        assert!(kinds(&r).contains(&ToothFlagKind::Negated), "{r:?}");
        assert!(check(&charted("504", "extraction", Some("closed"), None, &[5]), &t).flags.is_empty());
        // "Absent" is the missing finding's own word.
        let t = transcript(&["Two oh nine is absent."]);
        assert!(check(&charted("209", "missing", None, None, &[0]), &t).flags.is_empty());
    }

    #[test]
    fn a_planned_treatment_charted_as_done_is_flagged() {
        let t = transcript(&[
            "Three oh four may need to come out. We'll radiograph it again at the recheck.",
            "Two oh four doesn't need to come out.",
            "We'll extract four oh eight at the next visit.",
            "Three oh nine is extracted.",
            "The lower incisors come out closed, no sectioning.",
        ]);
        let done = |tooth: &str, line: u32| charted(tooth, "extraction", None, None, &[line]);
        assert_eq!(kinds(&check(&done("304", 0), &t)), [ToothFlagKind::Planned]);
        assert!(kinds(&check(&done("204", 1), &t)).contains(&ToothFlagKind::Negated));
        assert_eq!(kinds(&check(&done("408", 2), &t)), [ToothFlagKind::Planned]);
        assert!(check(&done("309", 3), &t).flags.is_empty());
        assert!(check(&done("301", 4), &t).flags.is_empty());
        let mut planned = done("304", 0);
        planned["planned"] = json!(true);
        assert!(check(&planned, &t).flags.is_empty(), "charted as a plan");
    }

    #[test]
    fn a_name_conflict_is_flagged_whichever_tooth_was_charted() {
        let t = transcript(&[
            "One oh eight, upper left fourth premolar, has a slab fracture with pulp exposure.",
        ]);
        let by_name = check(&charted("208", "fracture", Some("complicated_crown"), None, &[0]), &t);
        assert_eq!(
            by_name.flags.iter().map(|f| (f.kind, f.other.as_deref())).collect::<Vec<_>>(),
            [(ToothFlagKind::NameConflict, Some("108"))]
        );
        // The hint leaves out the name that disagrees with its number.
        assert_eq!(hint_teeth("One oh eight, upper left fourth premolar, has a slab fracture."), ["108"]);
        assert_eq!(hint_teeth("Four oh four, the lower right canine, looks fine."), ["404"]);
        assert_eq!(
            hint_teeth("All the lower incisors are mobile."),
            ["301", "302", "303", "401", "402", "403"]
        );
    }

    #[test]
    fn a_group_said_elsewhere_never_softens_an_invented_tooth() {
        let t = transcript(&["Mild calculus on the upper premolars.", "Two oh eight is fractured."]);
        let r = check(&charted("206", "fracture", None, None, &[1]), &t);
        assert_eq!(r.unsupported.len(), 1, "{r:?}");
        assert!(r.mismatches.is_empty());
        // A number said elsewhere is still a citation mismatch.
        let t = transcript(&["Two oh six is fractured.", "Two oh eight is fractured."]);
        let r = check(&charted("206", "fracture", None, None, &[1]), &t);
        assert_eq!((r.unsupported.len(), r.mismatches.len()), (0, 1));
    }

    #[test]
    fn counts_vague_amounts_and_negations_name_no_teeth() {
        for text in [
            "One of the lower incisors is missing.",
            "Two lower incisors are missing.",
            "Two of the lower incisors are missing.",
            "Some of the lower incisors are mobile.",
            "A couple of the upper premolars have calculus.",
            "A few premolars are mobile.",
            "Four of the premolars are mobile.",
            "None of the canines are fractured.",
            "No fractured canines.",
            "Neither lower canine is fractured.",
            "All the other premolars look normal.",
            "The remaining lower premolars are fine.",
            "Calculus on the premolars, otherwise unremarkable.",
            "The adult canines look normal.",
            "The upper right and lower left canines are fractured.",
        ] {
            assert!(teeth_in(text).is_empty(), "{text}: {:?}", teeth_in(text));
        }
        assert_eq!(read("Both lower canines are fractured."), ["304", "404"]);
        assert_eq!(read("Both lower incisors"), Vec::<String>::new(), "both is two teeth");
        assert_eq!(read("The six lower incisors are mobile."), ["301", "302", "303", "401", "402", "403"]);
    }

    #[test]
    fn side_and_jaw_after_the_tooth_word_and_names_said_together() {
        let cases: &[(&str, &[&str])] = &[
            ("The fourth premolar on the upper right is fractured.", &["108"]),
            ("First molar, lower left, is mobile.", &["309"]),
            ("The canine on the lower right is fine.", &["404"]),
            ("The right first molar, lower jaw, is fractured.", &["409"]),
            ("Her lower left first molar and second molar are mobile.", &["309", "310"]),
            ("The right maxillary fourth premolar and first molar are mobile.", &["108", "109"]),
            ("The upper right canine and premolars are fine.", &["104", "105", "106", "107", "108"]),
            ("Fourth premolars on the right side look fine.", &["108", "408"]),
            // The side and jaw lead into the next tooth's name, not back.
            ("The upper left canine, lower right fourth premolar.", &["204", "408"]),
        ];
        for (text, expected) in cases {
            assert_eq!(read(text), *expected, "{text}");
        }
        // Correct items on those teeth pass the tooth check.
        let t = transcript(&[
            "The fourth premolar on the upper right is fractured.",
            "Her lower left first molar and second molar are mobile.",
        ]);
        assert!(check(&charted("108", "fracture", None, None, &[0]), &t).unsupported.is_empty());
        let r = check(&charted("310", "mobility", None, None, &[1]), &t);
        assert!(r.unsupported.is_empty() && r.flags.is_empty(), "{r:?}");
    }

    #[test]
    fn the_mouth_decides_which_teeth_can_be_charted() {
        let t = transcript(&[
            "The upper right first premolar is fractured.",
            "One oh five and three oh five are missing.",
        ]);
        let item = charted("305", "missing", None, None, &[1]);
        let map = item.as_object().unwrap();
        // No dentition recorded: a tooth only a dog has blocks signing.
        let r = check_item(map, "/dental_chart/0", &t, None);
        assert_eq!(kinds(&r), [ToothFlagKind::ChooseDentition]);
        assert!(ToothFlagKind::ChooseDentition.blocks_signing());
        // A cat: not in its mouth, blocks signing too.
        let r = check_item(map, "/dental_chart/0", &t, Some(Dentition::Feline));
        assert_eq!(kinds(&r), [ToothFlagKind::NotInMouth]);
        // A dog: fine. A cat's tooth with no dentition: fine.
        assert!(check_item(map, "/dental_chart/0", &t, Some(Dentition::Canine)).flags.is_empty());
        let shared = charted("309", "missing", None, None, &[1]);
        assert!(
            !kinds(&check_item(shared.as_object().unwrap(), "/x", &t, None))
                .contains(&ToothFlagKind::ChooseDentition)
        );
        // The vet's own tooth too.
        let mut vet = charted("105", "missing", None, None, &[]);
        vet["entered_by"] = json!("Dr Okafor");
        let r = check_item(vet.as_object().unwrap(), "/dental_chart/0", &t, Some(Dentition::Feline));
        assert_eq!((r.checked, kinds(&r)), (0, vec![ToothFlagKind::NotInMouth]));
    }

    #[test]
    fn findings_that_contradict_each_other_on_one_tooth_are_flagged() {
        let conflict_paths = |items: Vec<Value>| -> Vec<(String, Option<String>)> {
            duplicates(&items, "/dental_chart")
                .into_iter()
                .filter(|f| f.kind == ToothFlagKind::Conflict)
                .map(|f| (f.path, f.other))
                .collect()
        };
        // Normal beside a fracture.
        let c = conflict_paths(vec![
            charted("108", "normal", None, None, &[0]),
            charted("108", "fracture", None, None, &[0]),
        ]);
        assert_eq!(c, [("/dental_chart/1".to_string(), Some("/dental_chart/0".to_string()))]);
        // Two resorption stages.
        let c = conflict_paths(vec![
            charted("307", "resorption", Some("crown_worse"), Some(4), &[0]),
            charted("307", "resorption", None, Some(2), &[1]),
        ]);
        assert_eq!(c.len(), 1);
        // Missing beside an extraction done; not beside a planned one.
        assert_eq!(
            conflict_paths(vec![
                charted("209", "missing", None, None, &[0]),
                charted("209", "extraction", None, None, &[0]),
            ])
            .len(),
            1
        );
        let mut planned = charted("209", "extraction", None, None, &[0]);
        planned["planned"] = json!(true);
        assert!(conflict_paths(vec![charted("209", "missing", None, None, &[0]), planned]).is_empty());
        // Two extraction techniques.
        assert_eq!(
            conflict_paths(vec![
                charted("108", "extraction", Some("open"), None, &[0]),
                charted("108", "extraction", Some("closed"), None, &[0]),
            ])
            .len(),
            1
        );
        // Different findings that can sit together, and other teeth.
        assert!(
            conflict_paths(vec![
                charted("108", "fracture", Some("complicated_crown"), None, &[0]),
                charted("108", "extraction", Some("open"), None, &[0]),
                charted("208", "normal", None, None, &[0]),
                charted("309", "mobility", None, Some(2), &[0]),
                charted("309", "periodontal_stage", None, Some(3), &[0]),
            ])
            .is_empty()
        );
        // An exact repeat is a duplicate, not a conflict.
        let flags = duplicates(
            &[
                charted("309", "mobility", None, Some(2), &[0]),
                charted("309", "mobility", None, Some(2), &[0]),
            ],
            "/dental_chart",
        );
        assert_eq!(flags.iter().map(|f| f.kind).collect::<Vec<_>>(), [ToothFlagKind::Duplicate]);
    }

    #[test]
    fn findings_fit_the_kind_of_tooth() {
        let pd = |tooth: &str| item(tooth, "persistent_deciduous", None, None, None, false);
        assert!(matches!(fits(&pd("104")), Err(Misfit::WrongKind { kind: ToothKind::Deciduous, .. })));
        assert_eq!(fits(&pd("504")), Ok(()));
        assert_eq!(
            fits(&pd("304")).unwrap_err().to_string(),
            "Persistent deciduous tooth is only for deciduous teeth (500 to 800), not 304."
        );
        assert!(matches!(
            fits(&item("704", "unerupted", None, None, None, false)),
            Err(Misfit::WrongKind { kind: ToothKind::Permanent, .. })
        ));
        // The check flags it as a shape problem.
        let t = transcript(&[
            "She has persistent deciduous upper canines, five oh four and six oh four, beside the adult canines.",
        ]);
        let r = check(&charted("104", "persistent_deciduous", None, None, &[0]), &t);
        assert!(r.unsupported.len() == 1 || kinds(&r).contains(&ToothFlagKind::Shape), "{r:?}");
        assert!(kinds(&check(&charted("504", "persistent_deciduous", None, None, &[0]), &t)).is_empty());
    }

    /// Signed runs record the finding table's version (`RunInfo::chart_version`):
    /// any change to a finding, detail, word or code must bump `version`, and
    /// keep the old table for drawing older runs. Update both together.
    #[test]
    fn the_finding_table_changes_only_with_its_version() {
        use sha2::{Digest, Sha256};
        let v: Value = serde_json::from_str(CHART_JSON).unwrap();
        let digest = hex::encode(Sha256::digest(v["findings"].to_string().as_bytes()));
        assert_eq!(
            (chart().version, digest.as_str()),
            (1, "d8260ada4c633b6f27d5b8e14fc0074c86d06a5cb08d290e34504d3d075ef3b8"),
            "the finding table changed: bump its version"
        );
    }
}
