//! PDF export of a note (M1 item 12): clinic letterhead, visit details,
//! signature status, every heading (with "Not discussed" where nothing was
//! said), addenda and page numbers.
//!
//! A small writer on the PDF standard fonts (Helvetica, WinAnsi encoding):
//! no font files, no dependencies, identical output on every platform.
//! Citations are left out; the signed record reads as a clean document.

use serde::Deserialize;
use serde_json::Value;

use crate::render::field_text;
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
}

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
    Text { x: f32, y: f32, font: Font, size: f32, grey: f32, text: String },
    Rule { y: f32, grey: f32 },
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
}

const BODY: f32 = 10.5;

fn statements(l: &mut Layout, value: &Value, indent: f32) {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
    }
    for item in items {
        l.text(item["text"].as_str().unwrap_or_default(), Font::Regular, BODY, 0.0, indent, Some("•"));
    }
}

/// A text box: its sentences as one paragraph.
fn paragraph(l: &mut Layout, value: &Value, indent: f32) {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
        return;
    }
    let text: Vec<&str> = items.iter().filter_map(|i| i["text"].as_str()).collect();
    l.text(&text.join(" "), Font::Regular, BODY, 0.0, indent, None);
}

fn medications(l: &mut Layout, value: &Value, indent: f32) {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        l.text(l.labels.not_discussed, Font::Italic, BODY, 0.4, indent, None);
    }
    for med in items {
        let details: Vec<&str> =
            ["dose", "route", "frequency", "duration"].iter().filter_map(|k| med[*k].as_str()).collect();
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

fn content(l: &mut Layout, kind: PartKind, defs: &[FieldDef], value: &Value, indent: f32) {
    match kind {
        PartKind::Statements => statements(l, value, indent),
        PartKind::Text => paragraph(l, value, indent),
        PartKind::Medications => medications(l, value, indent),
        PartKind::Fields => fields(l, defs, value, indent),
    }
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
            Some(kind) => content(&mut l, kind, &section.fields, value, 0.0),
            None => {
                for part in &section.parts {
                    l.ensure(BODY * 3.0);
                    l.gap(3.0);
                    l.text(&part.label, Font::Bold, BODY, 0.3, 0.0, None);
                    content(&mut l, part.kind, &part.fields, &value[&part.key], 0.0);
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
