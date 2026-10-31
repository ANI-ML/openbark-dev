//! PDF export of a note (M1 item 12): clinic letterhead, visit details,
//! signature status, every heading (with "Not discussed" where nothing was
//! said), addenda and page numbers.
//!
//! A small writer on the PDF standard fonts (Helvetica, WinAnsi encoding):
//! no font files, no dependencies, identical output on every platform.
//! Citations are left out; the signed record reads as a clean document.
//!
//! Tooth charts (ADR-0021 §9) are drawn with a path operator (move, line,
//! curve, close, stroke, fill, dash) from the outlines in
//! `packages/dental/chart.json`, the same data the app draws from.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::dental::{self, Dentition, Glyph, LegendLine, PathCmd};
use crate::render::{field_text, medication_details, shown_items, shown_medications};
use crate::template::{FieldDef, LoadedTemplate, PartKind};

/// Clinic details printed as the letterhead.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Clinic {
    #[serde(default)]
    pub name: String,
    /// Address, phone, email: one per line.
    #[serde(default)]
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    #[default]
    Letter,
    A4,
}

impl PageSize {
    fn points(self) -> (f32, f32) {
        match self {
            PageSize::Letter => (612.0, 792.0),
            PageSize::A4 => (595.0, 842.0),
        }
    }
}

/// The fixed words on a PDF, in the app's languages.
#[derive(Debug, Clone, Copy)]
pub struct Labels {
    pub patient: &'static str,
    pub reason: &'static str,
    pub recorded: &'static str,
    /// "Signed by {name}, {at}".
    pub signed_by: &'static str,
    pub addendum: &'static str,
    pub not_discussed: &'static str,
    /// "(dose, route and frequency not stated)".
    pub not_stated: &'static str,
    pub draft_record: &'static str,
    pub draft_client: &'static str,
    /// "Page {i} of {n}".
    pub page: &'static str,
    /// The tooth chart's words (finding words, captions, legend).
    pub dental: &'static DentalWords,
}

/// The tooth chart's words on a PDF, by their English text (which is also the
/// app's translation key). They come from the app's own translation files
/// (packages/i18n), so the screen and the signed PDF word a finding the same
/// way. A word without an entry prints in English; AVDC codes are never
/// translated. Every translation is listed for review by a native-speaking
/// vet before release (ADR-0021 §11).
#[derive(Debug)]
pub struct DentalWords {
    /// Lowercase the first letter of words inside a line ("Planned: open
    /// extraction"; "fracture; open extraction"). Not in German.
    pub lower_inner: bool,
    /// The interface language whose translations are used ("en": none).
    pub lang: &'static str,
}

impl DentalWords {
    pub fn tr(&self, english: &str) -> String {
        dental_catalog(self.lang).get(english).cloned().unwrap_or_else(|| english.to_string())
    }
}

/// The chart's fixed words on a PDF; the finding words and legend lines come
/// from `chart.json`.
pub const DENTAL_FIXED_WORDS: &[&str] = &[
    "Planned: {finding}",
    "Patient's right",
    "Patient's left",
    "Blank: not charted in this recording. It doesn't mean normal.",
    "Right and left are the patient's.",
    "Dentition not recorded.",
];

/// Every word a tooth chart prints: the fixed words, and each finding's
/// words, legend and type words from `chart.json`.
pub fn dental_keys() -> Vec<String> {
    let chart = dental::chart();
    let mut keys: Vec<String> = DENTAL_FIXED_WORDS.iter().map(|s| s.to_string()).collect();
    for f in &chart.findings {
        keys.push(f.words.clone());
        keys.extend([&f.words_grade, &f.words_mm, &f.legend].into_iter().flatten().cloned());
        keys.extend(f.details.iter().filter(|d| d.with_grade.is_none()).map(|d| d.words.clone()));
    }
    keys.sort();
    keys.dedup();
    keys
}

const I18N: &[(&str, &str)] = &[
    ("fr", include_str!("../../../packages/i18n/i18n-fr.json")),
    ("es", include_str!("../../../packages/i18n/i18n-es.json")),
    ("de", include_str!("../../../packages/i18n/i18n-de.json")),
    ("pt", include_str!("../../../packages/i18n/i18n-pt.json")),
];

/// One language's chart words, read once from the app's translation file.
fn dental_catalog(lang: &str) -> &'static HashMap<String, String> {
    use std::sync::LazyLock;
    static CATALOGS: LazyLock<HashMap<&'static str, HashMap<String, String>>> = LazyLock::new(|| {
        let keys = dental_keys();
        I18N.iter()
            .map(|(lang, json)| {
                let all: HashMap<String, String> = serde_json::from_str(json).unwrap_or_default();
                let words =
                    all.into_iter().filter(|(k, v)| keys.contains(k) && !v.trim().is_empty()).collect();
                (*lang, words)
            })
            .collect()
    });
    static NONE: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);
    CATALOGS.get(lang).unwrap_or(&NONE)
}

const DENTAL_EN: DentalWords = DentalWords { lower_inner: true, lang: "en" };
const DENTAL_FR: DentalWords = DentalWords { lower_inner: true, lang: "fr" };
const DENTAL_ES: DentalWords = DentalWords { lower_inner: true, lang: "es" };
const DENTAL_DE: DentalWords = DentalWords { lower_inner: false, lang: "de" };
const DENTAL_PT: DentalWords = DentalWords { lower_inner: true, lang: "pt" };

const LABELS: &[(&str, Labels)] = &[
    (
        "en",
        Labels {
            patient: "Patient",
            reason: "Reason for visit",
            recorded: "Recorded",
            signed_by: "Signed by {name}, {at}",
            addendum: "Addendum",
            not_discussed: "Not discussed",
            not_stated: "(dose, route and frequency not stated)",
            draft_record: "DRAFT: not reviewed or signed by a veterinarian.",
            draft_client: "Draft for the client: check it before handing it over.",
            page: "Page {i} of {n}",
            dental: &DENTAL_EN,
        },
    ),
    (
        "fr",
        Labels {
            patient: "Patient",
            reason: "Motif de consultation",
            recorded: "Enregistré le",
            signed_by: "Signé par {name}, {at}",
            addendum: "Addenda",
            not_discussed: "Non abordé",
            not_stated: "(dose, voie et fréquence non précisées)",
            draft_record: "BROUILLON : non relu ni signé par un vétérinaire.",
            draft_client: "Brouillon pour le client : à relire avant de le remettre.",
            page: "Page {i} sur {n}",
            dental: &DENTAL_FR,
        },
    ),
    (
        "es",
        Labels {
            patient: "Paciente",
            reason: "Motivo de consulta",
            recorded: "Grabado",
            signed_by: "Firmado por {name}, {at}",
            addendum: "Adenda",
            not_discussed: "No tratado",
            not_stated: "(dosis, vía y frecuencia no indicadas)",
            draft_record: "BORRADOR: no revisado ni firmado por un veterinario.",
            draft_client: "Borrador para el cliente: revíselo antes de entregarlo.",
            page: "Página {i} de {n}",
            dental: &DENTAL_ES,
        },
    ),
    (
        "de",
        Labels {
            patient: "Patient",
            reason: "Vorstellungsgrund",
            recorded: "Aufgenommen",
            signed_by: "Unterschrieben von {name}, {at}",
            addendum: "Nachtrag",
            not_discussed: "Nicht besprochen",
            not_stated: "(Dosis, Gabe und Häufigkeit nicht genannt)",
            draft_record: "ENTWURF: nicht von einem Tierarzt geprüft oder unterschrieben.",
            draft_client: "Entwurf für den Kunden: vor der Übergabe prüfen.",
            page: "Seite {i} von {n}",
            dental: &DENTAL_DE,
        },
    ),
    (
        "pt",
        Labels {
            patient: "Paciente",
            reason: "Motivo da consulta",
            recorded: "Gravado",
            signed_by: "Assinado por {name}, {at}",
            addendum: "Adenda",
            not_discussed: "Não abordado",
            not_stated: "(dose, via e frequência não indicadas)",
            draft_record: "RASCUNHO: não revisto nem assinado por um veterinário.",
            draft_client: "Rascunho para o cliente: reveja antes de entregar.",
            page: "Página {i} de {n}",
            dental: &DENTAL_PT,
        },
    ),
];

impl Labels {
    /// Labels for a language code ("fr"); English when there are none.
    pub fn for_language(code: &str) -> &'static Labels {
        let code = code.get(..2).unwrap_or(code);
        LABELS.iter().find(|(c, _)| *c == code).map(|(_, l)| l).unwrap_or(&LABELS[0].1)
    }

    pub fn signed(&self, name: &str, at: &str) -> String {
        self.signed_by.replace("{name}", name).replace("{at}", at)
    }
}

impl Default for &'static Labels {
    fn default() -> Self {
        Labels::for_language("en")
    }
}

/// Everything around the note itself.
#[derive(Debug, Clone, Default)]
pub struct PdfHeader {
    pub clinic: Clinic,
    /// "Patient: Miso Tanaka", "Reason for visit: Vomiting", "Recorded: …".
    pub details: Vec<String>,
    /// "Signed by Dr Patel, 2026-10-17 21:15", or None for a draft.
    pub signed: Option<String>,
    /// (heading, text) pairs, e.g. ("Addendum, 2026-10-18 09:12, Dr Patel", "…").
    pub addenda: Vec<(String, String)>,
    pub page: PageSize,
    pub labels: &'static Labels,
}

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Regular,
    Bold,
    Italic,
}

impl Font {
    fn name(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Italic => "F3",
        }
    }
}

/// Helvetica and Helvetica-Bold advance widths (1/1000 em) for ASCII 32-126,
/// from the standard AFM files. Oblique shares Helvetica's widths.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278,
    500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469,
    556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278,
    556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584,
    556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556,
    333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// A character as a WinAnsi byte (`?` when it has none).
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        '\t' => b' ',
        _ => b'?',
    }
}

fn width(text: &str, font: Font, size: f32) -> f32 {
    let table = if font == Font::Bold { &HELVETICA_BOLD } else { &HELVETICA };
    text.chars()
        .map(|c| match win_ansi(c) {
            b @ 32..=126 => table[(b - 32) as usize] as f32,
            0xb0 => 400.0, // degree sign
            _ => 556.0,
        })
        .sum::<f32>()
        * size
        / 1000.0
}

/// Break text into lines no wider than `max`.
fn wrap(text: &str, font: Font, size: f32, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if width(&candidate, font, size) <= max || line.is_empty() {
                // A single over-long word is hard-broken below.
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            while width(&line, font, size) > max && line.chars().count() > 1 {
                let mut cut = line.chars().count() - 1;
                while cut > 1 && width(&line.chars().take(cut).collect::<String>(), font, size) > max {
                    cut -= 1;
                }
                let head: String = line.chars().take(cut).collect();
                lines.push(head);
                line = line.chars().skip(cut).collect();
            }
        }
        lines.push(line);
    }
    lines
}

fn escape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in text.chars() {
        match win_ansi(c) {
            b'(' => out.extend(b"\\("),
            b')' => out.extend(b"\\)"),
            b'\\' => out.extend(b"\\\\"),
            b => out.push(b),
        }
    }
    out
}

/// One drawing operation on a page.
enum Op {
    Text {
        x: f32,
        y: f32,
        font: Font,
        size: f32,
        grey: f32,
        text: String,
    },
    Rule {
        y: f32,
        grey: f32,
    },
    /// A path in page coordinates (points, y up), stroked and/or filled in grey.
    Path {
        cmds: Vec<PathCmd>,
        stroke: Option<f32>,
        fill: Option<f32>,
        width: f32,
        dash: bool,
    },
}

struct Layout {
    labels: &'static Labels,
    page: (f32, f32),
    margin: f32,
    y: f32,
    pages: Vec<Vec<Op>>,
}

const FOOTER: f32 = 36.0;

impl Layout {
    fn new(size: PageSize, labels: &'static Labels) -> Self {
        let page = size.points();
        Self { labels, page, margin: 54.0, y: page.1 - 54.0, pages: vec![Vec::new()] }
    }

    fn width(&self) -> f32 {
        self.page.0 - 2.0 * self.margin
    }

    fn ensure(&mut self, height: f32) {
        if self.y - height < self.margin + FOOTER {
            self.pages.push(Vec::new());
            self.y = self.page.1 - self.margin;
        }
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("at least one page")
    }

    fn gap(&mut self, h: f32) {
        self.y -= h;
    }

    fn rule(&mut self, grey: f32) {
        self.ensure(8.0);
        let y = self.y;
        self.ops().push(Op::Rule { y, grey });
        self.y -= 8.0;
    }

    /// Wrapped text at an indent, with a hanging prefix (a bullet) if given.
    fn text(&mut self, text: &str, font: Font, size: f32, grey: f32, indent: f32, bullet: Option<&str>) {
        let leading = size * 1.35;
        let hang = if bullet.is_some() { 12.0 } else { 0.0 };
        let max = self.width() - indent - hang;
        for (i, line) in wrap(text, font, size, max).into_iter().enumerate() {
            self.ensure(leading);
            self.y -= leading;
            let x = self.margin + indent;
            if i == 0
                && let Some(b) = bullet
            {
                let y = self.y;
                self.ops().push(Op::Text { x, y, font: Font::Regular, size, grey, text: b.to_string() });
            }
            let y = self.y;
            self.ops().push(Op::Text { x: x + hang, y, font, size, grey, text: line });
        }
    }

    /// A label and its value on one line, the value wrapping under itself.
    fn field(&mut self, label: &str, value: &str, size: f32, indent: f32) {
        let leading = size * 1.35;
        let label_w = 120.0_f32.min(self.width() * 0.35);
        let max = self.width() - indent - label_w;
        for (i, line) in wrap(value, Font::Regular, size, max).into_iter().enumerate() {
            self.ensure(leading);
            self.y -= leading;
            let x = self.margin + indent;
            let y = self.y;
            if i == 0 {
                self.ops().push(Op::Text {
                    x,
                    y,
                    font: Font::Regular,
                    size,
                    grey: 0.4,
                    text: label.to_string(),
                });
            }
            self.ops().push(Op::Text { x: x + label_w, y, font: Font::Regular, size, grey: 0.0, text: line });
        }
    }

    /// A short bold label ("108") with its text wrapping beside it.
    fn labelled(&mut self, label: &str, value: &str, size: f32, indent: f32, label_w: f32) {
        let leading = size * 1.35;
        let max = self.width() - indent - label_w;
        for (i, line) in wrap(value, Font::Regular, size, max).into_iter().enumerate() {
            self.ensure(leading);
            self.y -= leading;
            let (x, y) = (self.margin + indent, self.y);
            if i == 0 {
                self.ops().push(Op::Text {
                    x,
                    y,
                    font: Font::Bold,
                    size,
                    grey: 0.0,
                    text: label.to_string(),
                });
            }
            self.ops().push(Op::Text { x: x + label_w, y, font: Font::Regular, size, grey: 0.0, text: line });
        }
    }
}

const BODY: f32 = 10.5;

/// Statements, "null" left out ([`crate::render::shown_items`]).
fn statements(l: &mut Layout, value: &Value, indent: f32) {
    let items = shown_items(value);
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
    }
    for (_, _, text) in items {
        l.text(&text, Font::Regular, BODY, 0.0, indent, Some("•"));
    }
}

/// A text box: its sentences as one paragraph.
fn paragraph(l: &mut Layout, value: &Value, indent: f32) {
    let items = shown_items(value);
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
        return;
    }
    let text: Vec<String> = items.into_iter().map(|(_, _, text)| text).collect();
    l.text(&text.join(" "), Font::Regular, BODY, 0.0, indent, None);
}

fn medications(l: &mut Layout, value: &Value, indent: f32) {
    let items = shown_medications(value);
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
    }
    for (_, med) in items {
        let details = medication_details(med);
        let text = if details.is_empty() {
            format!("{} {}", med["drug"].as_str().unwrap_or("?"), l.labels.not_stated)
        } else {
            format!("{}: {}", med["drug"].as_str().unwrap_or("?"), details.join(", "))
        };
        l.text(&text, Font::Regular, BODY, 0.0, indent, Some("•"));
    }
}

fn fields(l: &mut Layout, defs: &[FieldDef], value: &Value, indent: f32) {
    let mut missing = Vec::new();
    let mut any = false;
    for def in defs {
        match field_text(&value[&def.key]) {
            Some(v) => {
                any = true;
                l.field(&def.label, &v, BODY, indent);
            }
            None => missing.push(def.label.as_str()),
        }
    }
    if !any {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
    } else if !missing.is_empty() {
        let text = format!("{}: {}", l.labels.not_discussed, missing.join(", "));
        l.text(&text, Font::Italic, BODY, 0.4, indent, None);
    }
}

fn content(
    l: &mut Layout,
    kind: PartKind,
    defs: &[FieldDef],
    dentition: Option<Dentition>,
    value: &Value,
    indent: f32,
) {
    match kind {
        PartKind::Statements => statements(l, value, indent),
        PartKind::Text => paragraph(l, value, indent),
        PartKind::Medications => medications(l, value, indent),
        PartKind::Fields => fields(l, defs, value, indent),
        PartKind::Teeth => tooth_chart(l, dentition, value, indent),
    }
}

// ---------------------------------------------------------------------------
// Tooth chart (ADR-0021 §9)
// ---------------------------------------------------------------------------

// The chart's vertical layout in screen pixels, as in the app, scaled to
// points on paper: captions, then rows of number band, tooth and mark band
// (mirrored for the lower jaw, so crowns face the bite line).
const CAPTION_H: f32 = 16.0;
const ROW_H: f32 = 64.0;
const NUMBER_BAND: f32 = 14.0;
const MARK_BAND: f32 = 10.0;
const ROW_GAP: f32 = 4.0;
const BITE_GAP: f32 = 8.0;
/// The widest slot on paper, in points, so a cat's 16 teeth stay modest.
const MAX_SLOT: f32 = 24.0;
const CHART_SMALL: f32 = 8.5;

/// An ellipse as four Bézier curves.
fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathCmd> {
    const K: f32 = 0.552_284_8;
    vec![
        PathCmd::Move(cx + rx, cy),
        PathCmd::Curve(cx + rx, cy + K * ry, cx + K * rx, cy + ry, cx, cy + ry),
        PathCmd::Curve(cx - K * rx, cy + ry, cx - rx, cy + K * ry, cx - rx, cy),
        PathCmd::Curve(cx - rx, cy - K * ry, cx - K * rx, cy - ry, cx, cy - ry),
        PathCmd::Curve(cx + K * rx, cy - ry, cx + rx, cy - K * ry, cx + rx, cy),
        PathCmd::Close,
    ]
}

/// A glyph's marks in the tooth's 20 x 40 box (crown at the top), each with
/// whether it is filled. The app draws the same marks
/// (apps/desktop/src/components/DentalChart.tsx).
fn glyph_marks(glyph: Glyph) -> Vec<(Vec<PathCmd>, bool)> {
    use PathCmd::{Close, Line, Move};
    let g = &dental::chart().geometry;
    let (neck, mid) = (g.neck, g.crown_mid);
    match glyph {
        Glyph::None => Vec::new(),
        // Missing: a circle around the tooth.
        Glyph::Circle => vec![(ellipse(10.0, 20.0, 14.0, 20.5), false)],
        // Extraction: an X through the tooth.
        Glyph::X => vec![(vec![Move(1.0, 1.0), Line(19.0, 39.0), Move(19.0, 1.0), Line(1.0, 39.0)], false)],
        // Fracture: a zigzag across the crown.
        Glyph::Zigzag => vec![(
            vec![
                Move(1.0, mid),
                Line(4.0, mid - 3.0),
                Line(7.0, mid + 3.0),
                Line(10.0, mid - 3.0),
                Line(13.0, mid + 3.0),
                Line(16.0, mid - 3.0),
                Line(19.0, mid),
            ],
            false,
        )],
        // Resorption: notches at the neck.
        Glyph::Notch => vec![(
            vec![
                Move(0.0, neck - 3.0),
                Line(5.0, neck),
                Line(0.0, neck + 3.0),
                Close,
                Move(20.0, neck - 3.0),
                Line(15.0, neck),
                Line(20.0, neck + 3.0),
                Close,
            ],
            true,
        )],
        // Crown amputation: a line across the neck.
        Glyph::Neck => vec![(vec![Move(-2.0, neck), Line(22.0, neck)], false)],
        // Any other finding: a small dot on the bite side of the crown.
        Glyph::Dot => vec![(ellipse(10.0, -5.0, 2.0, 2.0), true)],
    }
}

fn place(cmds: &[PathCmd], to_page: &dyn Fn(f32, f32) -> (f32, f32)) -> Vec<PathCmd> {
    cmds.iter()
        .map(|c| match *c {
            PathCmd::Move(x, y) => {
                let (x, y) = to_page(x, y);
                PathCmd::Move(x, y)
            }
            PathCmd::Line(x, y) => {
                let (x, y) = to_page(x, y);
                PathCmd::Line(x, y)
            }
            PathCmd::Curve(a, b, c, d, e, f) => {
                let ((a, b), (c, d), (e, f)) = (to_page(a, b), to_page(c, d), to_page(e, f));
                PathCmd::Curve(a, b, c, d, e, f)
            }
            PathCmd::Close => PathCmd::Close,
        })
        .collect()
}

/// Each glyph once per tooth; dashed only when every finding with it is a plan.
fn tooth_marks(found: &[&Value]) -> Vec<(Glyph, bool)> {
    let mut out: Vec<(Glyph, bool)> = Vec::new();
    for item in found {
        let glyph = dental::glyph_of(item);
        let planned = item["planned"].as_bool().unwrap_or(false);
        match out.iter_mut().find(|(g, _)| *g == glyph) {
            Some((_, p)) => *p = *p && planned,
            None => out.push((glyph, planned)),
        }
    }
    out
}

/// The schematic, seen from the front: patient's right on the viewer's left,
/// upper row 110-101 | 201-210, lower row 411-401 | 301-311, deciduous rows
/// between them when a deciduous tooth is charted. Greys only: uncharted
/// outlines light and thin, charted outlines and marks black, plans dashed.
fn tooth_diagram(l: &mut Layout, dentition: Dentition, items: &[Value], indent: f32) {
    let chart = dental::chart();
    let words = l.labels.dental;
    let mut found: HashMap<&str, Vec<&Value>> = HashMap::new();
    for item in items {
        if let Some(t) = item["tooth"].as_str() {
            found.entry(t).or_default().push(item);
        }
    }
    let deciduous = found.keys().any(|t| dental::tooth(t).is_some_and(|t| t.deciduous));
    let rows: Vec<(bool, &[String])> = chart
        .rows(dentition)
        .top_to_bottom()
        .into_iter()
        .filter(|(name, _)| deciduous || !name.ends_with("_deciduous"))
        .map(|(name, row)| (name.starts_with("upper"), row))
        .collect();
    let mut tops = Vec::new();
    let mut y = CAPTION_H;
    for (i, (upper, _)) in rows.iter().enumerate() {
        if i > 0 {
            y += if rows[i - 1].0 == *upper { ROW_GAP } else { BITE_GAP };
        }
        tops.push(y);
        y += ROW_H;
    }
    let height = y;
    let span = l.width() - indent;
    let most = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(1).max(1) as f32;
    let slot = (span / most).min(MAX_SLOT);
    let s = slot / chart.geometry.slot;
    // Keep the diagram with the start of its list.
    l.ensure(height * s + BODY * 1.35 * 3.0);
    l.gap(2.0);
    let top = l.y;
    let mid = l.margin + indent + span / 2.0;
    let page_y = |px: f32| top - px * s;

    for (text, centre) in
        [(words.tr("Patient's right"), mid - span / 4.0), (words.tr("Patient's left"), mid + span / 4.0)]
    {
        let x = centre - width(&text, Font::Regular, 7.5) / 2.0;
        l.ops().push(Op::Text { x, y: page_y(11.0), font: Font::Regular, size: 7.5, grey: 0.35, text });
    }
    for ((upper, row), row_top) in rows.iter().zip(&tops) {
        let upper = *upper;
        let on_right =
            row.iter().filter(|id| dental::tooth(id).is_some_and(|t| t.side == dental::Side::Right)).count();
        for (i, id) in row.iter().enumerate() {
            let Some(t) = dental::tooth(id) else { continue };
            let cx = if i < on_right {
                mid - ((on_right - 1 - i) as f32 + 0.5) * slot
            } else {
                mid + ((i - on_right) as f32 + 0.5) * slot
            };
            let box_top = row_top + if upper { NUMBER_BAND } else { MARK_BAND };
            let to_page = |x: f32, y: f32| {
                let down = if upper { chart.geometry.tooth[1] - y } else { y };
                (cx + (x - chart.geometry.tooth[0] / 2.0) * s, page_y(box_top + down))
            };
            let here = found.get(id.as_str()).map(Vec::as_slice).unwrap_or_default();
            let outline = dental::parse_path(chart.shapes.get(t.shape)).unwrap_or_default();
            let (grey, line) = if here.is_empty() { (0.6, 0.5) } else { (0.0, 0.9) };
            l.ops().push(Op::Path {
                cmds: place(&outline, &to_page),
                stroke: Some(grey),
                fill: None,
                width: line,
                dash: false,
            });
            for (glyph, planned) in tooth_marks(here) {
                for (cmds, filled) in glyph_marks(glyph) {
                    l.ops().push(Op::Path {
                        cmds: place(&cmds, &to_page),
                        stroke: (!filled).then_some(0.0),
                        fill: filled.then_some(0.0),
                        width: 0.9,
                        dash: planned && !filled,
                    });
                }
            }
            let baseline = if upper { row_top + 11.0 } else { row_top + ROW_H - 2.0 };
            let x = cx - width(id, Font::Regular, 6.5) / 2.0;
            l.ops().push(Op::Text {
                x,
                y: page_y(baseline),
                font: Font::Regular,
                size: 6.5,
                grey: 0.35,
                text: id.clone(),
            });
        }
    }
    l.ops().push(Op::Path {
        cmds: vec![PathCmd::Move(mid, page_y(CAPTION_H)), PathCmd::Line(mid, page_y(height))],
        stroke: Some(0.75),
        fill: None,
        width: 0.4,
        dash: false,
    });
    l.y = page_y(height) - 2.0;
}

/// One line per tooth in Triadan order: "108  Complicated crown fracture
/// (T/FX/CCF); open extraction (XSS)". No citations, as everywhere on paper.
fn tooth_list(l: &mut Layout, items: &[Value], indent: f32) {
    let words = l.labels.dental;
    let tr = |k: &str| words.tr(k);
    for (id, findings) in dental::by_tooth(items) {
        let parts: Vec<String> = findings
            .iter()
            .enumerate()
            .map(|(n, item)| {
                let text = dental::code_of(item).render(&tr, words.lower_inner);
                if n > 0 && words.lower_inner { dental::lower_first(&text) } else { text }
            })
            .collect();
        l.labelled(&id, &parts.join("; "), BODY, indent, 30.0);
    }
}

/// The codes used, in words, then (under a diagram) what blank teeth mean.
fn tooth_legend(l: &mut Layout, items: &[Value], indent: f32, drawn: bool) {
    let words = l.labels.dental;
    let mut lines: Vec<String> = dental::legend(items)
        .into_iter()
        .map(|line| match line {
            LegendLine::Index(text) => words.tr(text),
            LegendLine::Code { code, words: key } => {
                let w = words.tr(&key);
                format!("{code}: {}", if words.lower_inner { dental::lower_first(&w) } else { w })
            }
        })
        .collect();
    if drawn {
        lines.extend(
            [
                "Blank: not charted in this recording. It doesn't mean normal.",
                "Right and left are the patient's.",
            ]
            .map(|k| words.tr(k)),
        );
    }
    if !lines.is_empty() {
        l.gap(3.0);
    }
    for line in lines {
        l.text(&line, Font::Regular, CHART_SMALL, 0.35, indent, None);
    }
}

/// A tooth chart heading's content. Nothing charted prints "Not discussed"
/// and no diagram, so a blank chart is never read as a full normal exam. With
/// no dentition (species not entered), the findings print without a diagram.
fn tooth_chart(l: &mut Layout, dentition: Option<Dentition>, value: &Value, indent: f32) {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
        return;
    }
    if let Some(d) = dentition {
        tooth_diagram(l, d, items, indent);
    }
    tooth_list(l, items, indent);
    if dentition.is_none() {
        let text = l.labels.dental.tr("Dentition not recorded.");
        l.text(&text, Font::Italic, BODY, 0.4, indent, None);
    }
    tooth_legend(l, items, indent, dentition.is_some());
}

/// A page with one tooth chart under its heading (previews and tests). Notes
/// draw chart headings through the same code (`content`, `PartKind::Teeth`).
pub fn tooth_chart_pdf(
    title: &str,
    dentition: Option<Dentition>,
    items: &Value,
    header: &PdfHeader,
) -> Vec<u8> {
    let mut l = Layout::new(header.page, header.labels);
    l.text(title, Font::Bold, 12.5, 0.0, 0.0, None);
    tooth_chart(&mut l, dentition, items, 0.0);
    write(&l, title)
}

/// Lay out the note and write the PDF.
pub fn note_pdf(template: &LoadedTemplate, note: &Value, header: &PdfHeader) -> Vec<u8> {
    let mut l = Layout::new(header.page, header.labels);
    if !header.clinic.name.trim().is_empty() {
        l.text(header.clinic.name.trim(), Font::Bold, 15.0, 0.0, 0.0, None);
    }
    for line in header.clinic.lines.iter().filter(|s| !s.trim().is_empty()) {
        l.text(line.trim(), Font::Regular, 9.0, 0.4, 0.0, None);
    }
    if !header.clinic.name.trim().is_empty() || !header.clinic.lines.is_empty() {
        l.gap(6.0);
        l.rule(0.75);
    }
    l.gap(4.0);
    l.text(&template.template.name, Font::Bold, 17.0, 0.0, 0.0, None);
    l.gap(2.0);
    for d in &header.details {
        l.text(d, Font::Regular, 9.5, 0.25, 0.0, None);
    }
    match &header.signed {
        Some(s) => l.text(s, Font::Bold, 9.5, 0.0, 0.0, None),
        None => l.text(
            match template.template.document {
                crate::template::Document::Record => l.labels.draft_record,
                crate::template::Document::Client => l.labels.draft_client,
            },
            Font::Bold,
            9.5,
            0.0,
            0.0,
            None,
        ),
    }
    l.gap(6.0);
    l.rule(0.85);

    for section in &template.template.sections {
        l.ensure(BODY * 4.0); // keep a heading with its first lines
        l.gap(6.0);
        l.text(&section.title, Font::Bold, 12.5, 0.0, 0.0, None);
        let value = &note[&section.id];
        match section.kind {
            Some(kind) => content(&mut l, kind, &section.fields, section.dentition, value, 0.0),
            None => {
                for part in &section.parts {
                    l.ensure(BODY * 3.0);
                    l.gap(3.0);
                    l.text(&part.label, Font::Bold, BODY, 0.3, 0.0, None);
                    content(&mut l, part.kind, &part.fields, part.dentition, &value[&part.key], 0.0);
                }
            }
        }
    }
    for (heading, text) in &header.addenda {
        l.ensure(BODY * 4.0);
        l.gap(10.0);
        l.rule(0.85);
        l.text(heading, Font::Bold, 11.0, 0.0, 0.0, None);
        l.text(text, Font::Regular, BODY, 0.0, 0.0, None);
    }
    write(&l, &template.template.name)
}

fn write(l: &Layout, title: &str) -> Vec<u8> {
    let n = l.pages.len();
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1 catalog, 2 pages, 3-5 fonts, 6 info, then (page, content) pairs.
    let first_page = 7;
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", first_page + 2 * i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
    for base in ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique"] {
        objects.push(
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>")
                .into_bytes(),
        );
    }
    let mut info = b"<< /Producer (OpenBark) /Title (".to_vec();
    info.extend(escape(title));
    info.extend(b") >>");
    objects.push(info);
    for (i, ops) in l.pages.iter().enumerate() {
        let mut stream = Vec::new();
        for op in ops {
            match op {
                Op::Text { x, y, font, size, grey, text } => {
                    stream.extend(
                        format!("{grey:.2} g BT /{} {size:.1} Tf {x:.1} {y:.1} Td (", font.name()).as_bytes(),
                    );
                    stream.extend(escape(text));
                    stream.extend(b") Tj ET\n");
                }
                Op::Rule { y, grey } => {
                    let (x1, x2) = (l.margin, l.page.0 - l.margin);
                    stream.extend(
                        format!("{grey:.2} G 0.5 w {x1:.1} {y:.1} m {x2:.1} {y:.1} l S\n").as_bytes(),
                    );
                }
                Op::Path { cmds, stroke, fill, width, dash } => {
                    // Saved and restored around each path, so its colour,
                    // width and dash never leak into the text after it.
                    let mut s = String::from("q 1 J 1 j ");
                    if let Some(g) = stroke {
                        s.push_str(&format!("{g:.2} G {width:.2} w "));
                    }
                    if let Some(g) = fill {
                        s.push_str(&format!("{g:.2} g "));
                    }
                    if *dash {
                        s.push_str("[2 1.5] 0 d ");
                    }
                    for cmd in cmds {
                        match *cmd {
                            PathCmd::Move(x, y) => s.push_str(&format!("{x:.2} {y:.2} m ")),
                            PathCmd::Line(x, y) => s.push_str(&format!("{x:.2} {y:.2} l ")),
                            PathCmd::Curve(a, b, c, d, e, f) => {
                                s.push_str(&format!("{a:.2} {b:.2} {c:.2} {d:.2} {e:.2} {f:.2} c "))
                            }
                            PathCmd::Close => s.push_str("h "),
                        }
                    }
                    s.push_str(match (stroke, fill) {
                        (Some(_), Some(_)) => "B",
                        (Some(_), None) => "S",
                        (None, Some(_)) => "f",
                        (None, None) => "n",
                    });
                    s.push_str(" Q\n");
                    stream.extend(s.as_bytes());
                }
            }
        }
        let footer = l.labels.page.replace("{i}", &(i + 1).to_string()).replace("{n}", &n.to_string());
        let fx = l.page.0 - l.margin - width(&footer, Font::Regular, 8.5);
        stream.extend(format!("0.45 g BT /F1 8.5 Tf {fx:.1} {:.1} Td (", l.margin - 18.0).as_bytes());
        stream.extend(escape(&footer));
        stream.extend(b") Tj ET\n");
        let content_id = first_page + 2 * i + 1;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.0} {:.0}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {content_id} 0 R >>",
                l.page.0, l.page.1
            )
            .into_bytes(),
        );
        let mut content = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        content.extend(stream);
        content.extend(b"\nendstream");
        objects.push(content);
    }

    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(obj);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn header() -> PdfHeader {
        PdfHeader {
            clinic: Clinic { name: "Maple Street Animal Hospital".into(), lines: vec!["12 Maple St".into()] },
            details: vec!["Patient: Miso Tanaka".into(), "Recorded: 2026-10-17 20:07".into()],
            signed: Some("Signed by Dr Okafor, 2026-10-17 21:15".into()),
            addenda: vec![("Addendum, 2026-10-18 09:12, Dr Okafor".into(), "Eating again.".into())],
            page: PageSize::Letter,
            labels: Labels::for_language("en"),
        }
    }

    #[test]
    fn every_builtin_template_renders_a_pdf_with_its_headings() {
        for id in LoadedTemplate::builtin_ids() {
            let t = LoadedTemplate::builtin(id).unwrap();
            let note = crate::template::empty_note(&t.template);
            let pdf = note_pdf(&t, &note, &header());
            assert!(pdf.starts_with(b"%PDF-") && pdf.ends_with(b"%%EOF\n"), "{id}");
            let text = text_of(&pdf);
            for s in &t.template.sections {
                let shown = s.title.replace('(', "\\(").replace(')', "\\)");
                assert!(text.contains(&shown), "{id}: heading {} missing", s.title);
            }
        }
    }

    #[test]
    fn labels_follow_the_language() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let note = crate::template::empty_note(&soap.template);
        let pdf = note_pdf(
            &soap,
            &note,
            &PdfHeader { labels: Labels::for_language("fr-CA"), signed: None, ..header() },
        );
        let t = text_of(&pdf);
        assert!(t.contains("Non abord"), "{t}");
        assert!(t.contains("Page 1 sur"));
        assert!(pdf.windows(2).any(|w| w == [b'd', 0xe9]), "é written as WinAnsi");
        assert_eq!(Labels::for_language("xx").patient, "Patient");
        assert_eq!(Labels::for_language("de").signed("Dr P", "heute"), "Unterschrieben von Dr P, heute");
    }

    fn text_of(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).into_owned()
    }

    #[test]
    fn writes_a_well_formed_pdf_with_every_heading() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let mut note = crate::template::empty_note(&soap.template);
        note["objective"]["vitals"]["temperature"] = json!({"value": 39.1, "unit": "°C", "evidence": [20]});
        note["plan"]["medications"] = json!([
            {"drug": "Maropitant", "dose": "1 mg/kg", "route": "SC", "frequency": "once", "duration": null, "evidence": [34]}
        ]);
        note["plan"]["follow_up"] = json!([{"text": "Recheck (Sunday) in 3 days", "evidence": [41]}]);
        let pdf = note_pdf(&soap, &note, &header());
        let t = text_of(&pdf);
        assert!(t.starts_with("%PDF-1.4"));
        assert!(t.trim_end().ends_with("%%EOF"));
        for heading in ["Subjective", "Objective", "Assessment", "Plan", "Maple Street Animal Hospital"] {
            assert!(t.contains(heading), "{heading}");
        }
        assert!(t.contains("Maropitant: 1 mg/kg, SC, once"));
        assert!(t.contains("Recheck \\(Sunday\\) in 3 days"), "parentheses escaped");
        assert!(pdf.windows(5).any(|w| w == b"39.1 "), "the value is written");
        assert!(pdf.contains(&0xb0), "degree sign in WinAnsi");
        assert!(t.contains("Not discussed"));
        assert!(t.contains("Signed by Dr Okafor"));
        assert!(t.contains("Eating again."));
        assert!(!t.contains("[20]"), "no citations in the PDF");

        // The cross-reference table points at each object (byte offsets).
        let xref_at: usize = t.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref_at..].starts_with(b"xref"));
        let table = String::from_utf8(pdf[xref_at..].to_vec()).unwrap();
        let entries: Vec<usize> = table
            .lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .map(|l| l[..10].parse().unwrap())
            .collect();
        assert!(entries.len() >= 8);
        for (i, off) in entries.iter().enumerate() {
            assert!(pdf[*off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
        }
    }

    #[test]
    fn long_notes_flow_onto_more_pages() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let mut note = crate::template::empty_note(&soap.template);
        let many: Vec<Value> =
            (0..120).map(|i| json!({"text": format!("Finding number {i}, described at some length so it wraps onto a second line of the page when printed."), "evidence": [1]})).collect();
        note["objective"]["physical_exam"] = Value::Array(many);
        let pdf = note_pdf(&soap, &note, &PdfHeader { page: PageSize::A4, ..header() });
        let t = text_of(&pdf);
        let pages: usize =
            t.split("/Count ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert!(pages >= 4, "{pages}");
        assert!(t.contains(&format!("Page {pages} of {pages}")));
        assert!(t.contains("/MediaBox [0 0 595 842]"));
    }

    fn finding(
        tooth: &str,
        finding: &str,
        detail: Option<&str>,
        grade: Option<u8>,
        mm: Option<f64>,
        planned: bool,
    ) -> Value {
        json!({"tooth": tooth, "text": "as said", "finding": finding, "detail": detail, "grade": grade, "mm": mm, "planned": planned, "evidence": [4]})
    }

    /// A sample chart: a dog's dental, with a plan, a stage-4 resorption and
    /// periodontal findings on one tooth.
    fn sample_chart() -> Value {
        Value::Array(vec![
            finding("101", "missing", None, None, None, false),
            finding("104", "normal", None, None, None, false),
            finding("108", "fracture", Some("complicated_crown"), None, None, false),
            finding("108", "extraction", Some("open"), None, None, false),
            finding("304", "extraction", None, None, None, true),
            finding("304", "fracture", None, None, None, false),
            finding("307", "resorption", Some("crown_worse"), Some(4), None, false),
            finding("309", "periodontal_stage", None, Some(3), None, false),
            finding("309", "probing_depth", None, None, Some(7.0), false),
            finding("309", "mobility", None, Some(2), None, false),
            finding("404", "crown_amputation", None, None, None, false),
        ])
    }

    /// Chart path operators ("q 1 J" starts each one; rules don't use it).
    fn chart_paths(t: &str) -> usize {
        t.matches("q 1 J 1 j").count()
    }

    #[test]
    fn draws_a_sample_tooth_chart_with_paths_codes_and_legend() {
        let pdf = tooth_chart_pdf("Dental chart", Some(Dentition::Canine), &sample_chart(), &header());
        assert!(pdf.starts_with(b"%PDF-") && pdf.ends_with(b"%%EOF\n"));
        let t = text_of(&pdf);
        for needle in [
            " m ",
            " c ",
            " S Q",
            " f Q",
            "[2 1.5] 0 d",
            "Dental chart",
            "Patient's right",
            "Patient's left",
            "Missing",
            "Complicated crown fracture \\(T/FX/CCF\\); open extraction \\(XSS\\)",
            "Planned: extraction; fracture \\(T/FX\\)",
            "Tooth resorption stage 4b \\(TR4b\\)",
            "Periodontal disease stage 3 \\(PD3\\); probing depth 7 mm; mobility 2 \\(M2\\)",
            "Crown amputation \\(CR/A\\)",
            "T/FX/CCF: complicated crown fracture",
            "M0-M3: tooth mobility index \\(AVDC\\). Grades as said.",
            "Probing depth is in millimetres.",
            "Blank: not charted in this recording. It doesn't mean normal.",
            "Right and left are the patient's.",
        ] {
            assert!(t.contains(needle), "{needle}");
        }
        assert!(!t.contains("[4]"), "no citations in the PDF");
        assert!(!t.contains("(PD)") && !t.contains("PD7"), "pocket depth is never PD");
        // 42 permanent outlines, a midline and the marks; no deciduous row (none charted).
        assert!(chart_paths(&t) > 42 + 1, "{}", chart_paths(&t));
        assert!(t.contains("(110)") && t.contains("(411)") && !t.contains("(504)"));
    }

    #[test]
    fn deciduous_rows_appear_when_a_deciduous_tooth_is_charted() {
        let items = json!([finding("504", "persistent_deciduous", None, None, None, false)]);
        let t = text_of(&tooth_chart_pdf("Dental chart", Some(Dentition::Canine), &items, &header()));
        assert!(t.contains("(504)") && t.contains("(808)"));
        assert!(t.contains("DT/P: persistent deciduous tooth"));
        // 70 outlines (42 + 28), one dot, one midline.
        assert_eq!(chart_paths(&t), 70 + 1 + 1);
    }

    #[test]
    fn a_cat_chart_draws_a_cats_teeth() {
        let items = json!([finding("307", "resorption_type", None, Some(2), None, false)]);
        let t = text_of(&tooth_chart_pdf("Dental chart", Some(Dentition::Feline), &items, &header()));
        assert!(!t.contains("(105)") && !t.contains("(305)") && t.contains("(106)") && t.contains("(307)"));
        assert_eq!(chart_paths(&t), 30 + 1 + 1, "30 outlines, the notch and the midline");
        assert!(t.contains("TR1-TR5: tooth resorption stage"));
    }

    #[test]
    fn an_empty_chart_says_not_discussed_and_draws_nothing() {
        let t = text_of(&tooth_chart_pdf("Dental chart", Some(Dentition::Canine), &json!([]), &header()));
        assert!(t.contains("Not discussed"));
        assert_eq!(chart_paths(&t), 0);
        assert!(!t.contains("Blank: not charted") && !t.contains("Patient's right"));
    }

    #[test]
    fn no_dentition_lists_the_findings_without_a_diagram() {
        let t = text_of(&tooth_chart_pdf("Dental chart", None, &sample_chart(), &header()));
        assert!(t.contains("Dentition not recorded."));
        assert!(t.contains("T/FX/CCF") && t.contains("(108) Tj"));
        assert_eq!(chart_paths(&t), 0);
        assert!(!t.contains("Blank: not charted"), "no blank teeth to explain");
    }

    fn assert_inside(l: &Layout) {
        let (left, right) = (l.margin - 0.01, l.page.0 - l.margin + 0.01);
        let (bottom, top) = (l.margin + FOOTER - 0.01, l.page.1 - l.margin + 0.01);
        let mut points = 0;
        for op in l.pages.iter().flatten() {
            let Op::Path { cmds, .. } = op else { continue };
            for c in cmds {
                let pts: Vec<(f32, f32)> = match *c {
                    PathCmd::Move(x, y) | PathCmd::Line(x, y) => vec![(x, y)],
                    PathCmd::Curve(a, b, c, d, e, f) => vec![(a, b), (c, d), (e, f)],
                    PathCmd::Close => vec![],
                };
                for (x, y) in pts {
                    points += 1;
                    assert!(
                        (left..=right).contains(&x) && (bottom..=top).contains(&y),
                        "({x}, {y}) outside the margins"
                    );
                }
            }
        }
        assert!(points > 100);
    }

    #[test]
    fn the_diagram_stays_inside_the_margins_on_letter_and_a4() {
        let mut items = sample_chart();
        items.as_array_mut().unwrap().push(finding("411", "missing", None, None, None, false));
        items.as_array_mut().unwrap().push(finding("110", "extraction", Some("closed"), None, None, false));
        items.as_array_mut().unwrap().push(finding("808", "persistent_deciduous", None, None, None, false));
        for page in [PageSize::Letter, PageSize::A4] {
            let mut l = Layout::new(page, Labels::for_language("en"));
            tooth_chart(&mut l, Some(Dentition::Canine), &items, 0.0);
            assert_inside(&l);
            assert_eq!(l.pages.len(), 1);
        }
    }

    #[test]
    fn a_long_chart_flows_onto_a_second_page_with_the_diagram_on_the_first() {
        let ids: Vec<&str> = dental::ids(Some(Dentition::Canine)).into_iter().take(42).collect();
        let mut items = Vec::new();
        for (n, id) in ids.iter().cycle().take(60).enumerate() {
            items.push(match n % 3 {
                0 => finding(id, "mobility", None, Some(1), None, false),
                1 => finding(id, "probing_depth", None, None, Some(4.5), false),
                _ => finding(id, "gingival_recession", None, None, Some(2.0), false),
            });
        }
        let mut l = Layout::new(PageSize::Letter, Labels::for_language("en"));
        l.gap(560.0); // most of the page is taken by headings above the chart
        l.text("Dental chart", Font::Bold, 12.5, 0.0, 0.0, None);
        tooth_chart(&mut l, Some(Dentition::Canine), &Value::Array(items), 0.0);
        assert!(l.pages.len() >= 3, "{}", l.pages.len());
        let paths_on = |p: &Vec<Op>| p.iter().filter(|op| matches!(op, Op::Path { .. })).count();
        assert_eq!(paths_on(&l.pages[0]), 0, "no room for the diagram under the heading");
        assert!(paths_on(&l.pages[1]) > 42, "the whole diagram is on the next page");
        assert!(l.pages[2..].iter().all(|p| paths_on(p) == 0));
        // The start of the list (the bold "101") stays with the diagram.
        let list_starts = |p: &Vec<Op>| {
            p.iter().any(|op| matches!(op, Op::Text { text, font: Font::Bold, .. } if text == "101"))
        };
        assert!(list_starts(&l.pages[1]));
        assert_inside(&l);
    }

    #[test]
    fn every_pdf_language_has_every_dental_word() {
        let keys = dental_keys();
        let placeholders = |s: &str| {
            let mut p: Vec<String> =
                s.split('{').skip(1).filter_map(|x| x.split_once('}')).map(|(k, _)| k.to_string()).collect();
            p.sort();
            p
        };
        for lang in ["fr", "es", "de", "pt"] {
            let words = Labels::for_language(lang).dental;
            let catalog = dental_catalog(lang);
            for key in &keys {
                let v = catalog
                    .get(key)
                    .unwrap_or_else(|| panic!("{lang}: no translation of \"{key}\" in packages/i18n"));
                assert_eq!(placeholders(v), placeholders(key), "{lang}: {key}");
                assert!(v.chars().all(|c| win_ansi(c) != b'?' || c == '?'), "{lang}: {v} prints in WinAnsi");
                assert_eq!(&words.tr(key), v, "{lang}: the PDF uses the app's words");
            }
        }
        assert_eq!(Labels::for_language("en").dental.tr("Open extraction"), "Open extraction");
        // The same words as the app's screen, by construction.
        assert_eq!(
            Labels::for_language("de").dental.tr("Tooth resorption"),
            dental_catalog("de")["Tooth resorption"]
        );
    }

    #[test]
    fn chart_words_follow_the_language() {
        let pdf = tooth_chart_pdf(
            "Zahnschema",
            Some(Dentition::Canine),
            &sample_chart(),
            &PdfHeader { labels: Labels::for_language("de"), ..header() },
        );
        let t = text_of(&pdf);
        for needle in [
            "Komplizierte Kronenfraktur \\(T/FX/CCF\\); Offene Extraktion \\(XSS\\)",
            "Geplant: Extraktion",
            "Rechts \\(Patient\\)",
            "T/FX/CCF: Komplizierte Kronenfraktur",
        ] {
            assert!(t.contains(needle), "{needle}");
        }
        let fr = text_of(&tooth_chart_pdf(
            "Schéma dentaire",
            Some(Dentition::Canine),
            &sample_chart(),
            &PdfHeader { labels: Labels::for_language("fr"), ..header() },
        ));
        assert!(fr.contains("Pr") && fr.contains("vu : extraction"), "lowercased inside the line in French");
    }

    #[test]
    fn wrapping_respects_width_and_breaks_long_words() {
        let lines = wrap("a b c dddddddddddddddddddddddddddddddddddddddd", Font::Regular, 10.0, 60.0);
        assert!(
            lines.iter().all(|l| width(l, Font::Regular, 10.0) <= 60.0 || l.chars().count() == 1),
            "{lines:?}"
        );
        assert!(lines.len() > 2);
        assert_eq!(win_ansi('é'), 0xe9);
        assert_eq!(win_ansi('—'), 0x97);
        assert_eq!(win_ansi('漢'), b'?');
    }
}
