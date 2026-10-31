//! Blank values and "Label: value" text (ADR-0032).
//!
//! A small model sometimes fills a heading it has nothing for with the word
//! "null" ("Tartre : null", a dose of "n/a"). After drafting, such values
//! become real nulls and such statements are dropped ([`scrub_note`]), and
//! everything that shows a note (Review, Copy, the PDF, client documents)
//! skips what is left in notes drafted before that ([`shown_statement`],
//! [`shown_value`]). So "null" never reads as a finding.
//!
//! The same reader of "Label: value" text finds a filled-in exam pasted into
//! an instruction ([`crate::template_lint`]), in any language: it reads the
//! shape of the text, not its words.

use serde_json::Value;

/// Whole values that mean "nothing" ("null", "n/a"), compared trimmed and in
/// lower case. "none" is not here: "Vomiting: none" can be a negative that was
/// said, so only a whole value of "none" with no label is blank.
const BLANK_WORDS: &[&str] = &["null", "n/a", "na", "nil", "undefined", "-", "--", "—", "…", "...", "?"];

/// Is this whole value blank: empty, or a word that means nothing?
pub fn is_blank(text: &str) -> bool {
    let t = text.trim().trim_end_matches(['.', ',', ';']).trim();
    t.is_empty() || BLANK_WORDS.iter().any(|w| t.eq_ignore_ascii_case(w))
}

/// Is this a whole value that says nothing at all, "none" included? Used for
/// a statement with no label and a measurement's value, where "none" alone
/// isn't a finding.
pub fn is_blank_or_none(text: &str) -> bool {
    is_blank(text) || text.trim().trim_end_matches('.').trim().eq_ignore_ascii_case("none")
}

/// One "Label: value" piece of text: "Yeux : Normal, aucun écoulement", or
/// "T:" with an empty value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labelled {
    pub label: String,
    pub value: String,
}

/// Bullets that start an entry.
const BULLETS: &[char] = &['-', '•', '*', '–', '·'];

fn is_colon(c: char) -> bool {
    c == ':' || c == '：'
}

/// Could this be a label: one to five words, starting with a letter, with no
/// sentence punctuation, at most 40 characters?
fn is_label(label: &str) -> bool {
    let label = label.trim();
    let words = label.split_whitespace().count();
    (1..=5).contains(&words)
        && label.chars().count() <= 40
        && label.chars().next().is_some_and(char::is_alphabetic)
        && !label.contains([',', ';', '?', '!', '"', '“', '”'])
        && (!label.contains('.') || label.chars().count() <= 8)
}

/// Does `rest` start (after spaces) with a label and a colon?
fn starts_with_label(rest: &str) -> bool {
    let rest = rest.trim_start();
    match rest.char_indices().take(41).find(|(_, c)| is_colon(*c)) {
        Some((i, _)) => is_label(&rest[..i]),
        None => false,
    }
}

/// Split text into entries: lines, pieces after ";", and pieces that start
/// with a bullet followed by a label ("… -Tartre : -T: P: R:"). A dash inside
/// a value ("Normal - no discharge") doesn't start one.
fn entries(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.split(['\n', '\r', ';']) {
        let mut start = 0;
        let mut prev_space = true;
        for (i, c) in line.char_indices() {
            if BULLETS.contains(&c) && prev_space && starts_with_label(&line[i + c.len_utf8()..]) {
                if !line[start..i].trim().is_empty() {
                    out.push(&line[start..i]);
                }
                start = i + c.len_utf8();
            }
            prev_space = c.is_whitespace();
        }
        if !line[start..].trim().is_empty() {
            out.push(&line[start..]);
        }
    }
    out
}

/// Inside a value, where a short label and a colon start another piece ("T:
/// P: R:", "roses TRC: < 2 s"): a word of one to six letters right before a
/// colon, after a space.
fn split_points(value: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        if !is_colon(c) {
            continue;
        }
        // The word before the colon (spaces between are allowed).
        let mut end = k;
        while end > 0 && chars[end - 1].1 == ' ' {
            end -= 1;
        }
        let mut begin = end;
        while begin > 0 && (chars[begin - 1].1.is_alphanumeric() || chars[begin - 1].1 == '.') {
            begin -= 1;
        }
        let word: String = chars[begin..end].iter().map(|(_, c)| *c).collect();
        let before_ok = begin == 0 || chars[begin - 1].1.is_whitespace();
        let len = word.chars().count();
        if before_ok && (1..=6).contains(&len) && word.starts_with(char::is_alphabetic) {
            let at = chars.get(begin).map(|(b, _)| *b).unwrap_or(i);
            out.push((at, i + c.len_utf8()));
        }
    }
    out
}

/// The "Label: value" pieces of one entry, or None when it doesn't start with
/// a label.
fn entry_pieces(entry: &str) -> Option<Vec<Labelled>> {
    let e = entry.trim().trim_start_matches(BULLETS).trim();
    let (colon, c) = e.char_indices().find(|(_, c)| is_colon(*c))?;
    let label = e[..colon].trim();
    if !is_label(label) {
        return None;
    }
    let rest = &e[colon + c.len_utf8()..];
    let mut out = Vec::new();
    let mut current = label.to_string();
    let mut from = 0;
    for (at, after) in split_points(rest) {
        out.push(Labelled { label: current, value: rest[from..at].trim().to_string() });
        current = rest[at..after].trim().trim_end_matches(is_colon).trim().to_string();
        from = after;
    }
    out.push(Labelled { label: current, value: rest[from..].trim().to_string() });
    Some(out)
}

/// What [`label_values`] read: the pieces, and the entries that weren't any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabelledText {
    pub pieces: Vec<Labelled>,
    /// Entries with no label, in order, trimmed.
    pub other: Vec<String>,
}

/// Read text as "Label: value" pieces, by lines, bullets and short labels.
pub fn label_values(text: &str) -> LabelledText {
    let mut out = LabelledText::default();
    for entry in entries(text) {
        match entry_pieces(entry) {
            Some(pieces) => out.pieces.extend(pieces),
            None => out.other.push(entry.trim().to_string()),
        }
    }
    out
}

/// A statement as it should read, or None when it says nothing: "null",
/// "Tartre : null", "T: null P: null". Pieces with a blank value are left out
/// of a statement that has others ("T: 38.5 P: null" reads "T: 38.5").
pub fn shown_statement(text: &str) -> Option<String> {
    if is_blank_or_none(text) {
        return None;
    }
    // Only a statement made wholly of "Label: value" pieces is rewritten.
    if text.contains('\n') || !text.contains(is_colon) {
        return Some(text.to_string());
    }
    let read = label_values(text);
    if !read.other.is_empty() || read.pieces.is_empty() || !read.pieces.iter().any(|p| is_blank(&p.value)) {
        return Some(text.to_string());
    }
    let mut kept: Vec<String> = Vec::new();
    for p in read.pieces.iter().filter(|p| !is_blank(&p.value)) {
        kept.push(format!("{}: {}", p.label, p.value));
    }
    if kept.is_empty() { None } else { Some(kept.join(", ")) }
}

/// A measurement's or medication detail's text, or None when it says nothing.
pub fn shown_value(text: &str) -> Option<&str> {
    if is_blank_or_none(text) { None } else { Some(text) }
}

/// After drafting: turn blank values into nulls and drop statements that say
/// nothing. Statements are reworded only as [`shown_statement`] does; numbers
/// and evidence are untouched. Returns how many statements were dropped.
pub fn scrub_note(note: &mut Value) -> usize {
    let mut dropped = 0;
    scrub(note, &mut dropped);
    dropped
}

/// A measurement whose value is blank ("null", "n/a"): not discussed.
fn is_blank_field(v: &Value) -> bool {
    v.get("evidence").is_some()
        && v.get("unit").is_some()
        && v.get("value").and_then(Value::as_str).is_some_and(is_blank_or_none)
}

fn scrub(v: &mut Value, dropped: &mut usize) {
    if is_blank_field(v) {
        *v = Value::Null;
        return;
    }
    match v {
        Value::Array(items) => {
            let before = items.len();
            items.retain_mut(|item| {
                let Some(map) = item.as_object_mut() else { return true };
                if !map.contains_key("evidence") || map.contains_key("tooth") {
                    return true;
                }
                // A medication with no drug says nothing.
                if let Some(drug) = map.get("drug") {
                    return drug.as_str().is_some_and(|d| !is_blank_or_none(d));
                }
                match map.get("text").and_then(Value::as_str).map(shown_statement) {
                    Some(None) => false,
                    Some(Some(text)) => {
                        map.insert("text".into(), Value::String(text));
                        true
                    }
                    None => true,
                }
            });
            *dropped += before - items.len();
            for item in items.iter_mut() {
                scrub(item, dropped);
            }
        }
        Value::Object(map) => {
            for k in ["dose", "route", "frequency", "duration", "unit"] {
                if map.get(k).and_then(Value::as_str).is_some_and(is_blank_or_none) {
                    map.insert(k.into(), Value::Null);
                }
            }
            for (k, x) in map.iter_mut() {
                if k != "evidence" {
                    scrub(x, dropped);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn blank_words_are_blank_and_findings_are_not() {
        for blank in ["null", "NULL", " n/a ", "", "-", "…", "undefined", "null."] {
            assert!(is_blank(blank), "{blank:?}");
        }
        for said in ["Normal", "aucun écoulement", "none seen", "nil by mouth", "absent"] {
            assert!(!is_blank(said), "{said:?}");
        }
        assert!(is_blank_or_none("None") && !is_blank("None"));
    }

    #[test]
    fn pasted_exams_are_read_as_labelled_pieces() {
        let read = label_values("-Yeux : Normal, aucun écoulement -Tartre : -T: P: R: …");
        let labels: Vec<&str> = read.pieces.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Yeux", "Tartre", "T", "P", "R"]);
        assert_eq!(read.pieces[0].value, "Normal, aucun écoulement");
        assert_eq!(read.pieces[1].value, "");
        assert_eq!(read.pieces[4].value, "…");
        // A dash inside a value doesn't start an entry.
        let read = label_values("Eyes: normal - no discharge\nEars: clean");
        assert_eq!(read.pieces.len(), 2);
        assert_eq!(read.pieces[0].value, "normal - no discharge");
        // Prose has no pieces.
        let read = label_values("List the reason for the visit and the history.");
        assert!(read.pieces.is_empty());
        assert_eq!(read.other.len(), 1);
    }

    #[test]
    fn statements_that_say_nothing_are_dropped_or_trimmed() {
        assert_eq!(shown_statement("Tartre : null"), None);
        assert_eq!(shown_statement("T: null"), None);
        assert_eq!(shown_statement("T: null P: null R: null"), None);
        assert_eq!(shown_statement("null"), None);
        assert_eq!(shown_statement("None"), None);
        assert_eq!(shown_statement("T: 38.5 °C P: null").as_deref(), Some("T: 38.5 °C"));
        assert_eq!(shown_statement("Vomiting: none").as_deref(), Some("Vomiting: none"));
        assert_eq!(
            shown_statement("Yeux : écoulement séreux à gauche").as_deref(),
            Some("Yeux : écoulement séreux à gauche")
        );
        assert_eq!(shown_statement("Ate normally today").as_deref(), Some("Ate normally today"));
    }

    #[test]
    fn a_draft_loses_its_nulls() {
        let mut note = json!({
            "exam": [
                {"text": "Tartre : null", "evidence": [1]},
                {"text": "Yeux : écoulement séreux à gauche", "evidence": [2]},
                {"text": "T: null P: null", "evidence": [3]}
            ],
            "vitals": {
                "weight": {"value": "null", "unit": "kg", "evidence": [4]},
                "temperature": {"value": 38.5, "unit": "null", "evidence": [5]}
            },
            "plan": {"medications": [
                {"drug": "Meloxicam", "dose": "n/a", "route": "PO", "frequency": "null", "duration": null, "evidence": [6]},
                {"drug": "null", "dose": null, "route": null, "frequency": null, "duration": null, "evidence": [7]}
            ]}
        });
        assert_eq!(scrub_note(&mut note), 3);
        assert_eq!(note["exam"].as_array().unwrap().len(), 1);
        assert_eq!(note["vitals"]["weight"], Value::Null);
        assert_eq!(note["vitals"]["temperature"]["unit"], Value::Null);
        let meds = note["plan"]["medications"].as_array().unwrap();
        assert_eq!(meds.len(), 1);
        assert_eq!(meds[0]["dose"], Value::Null);
        assert_eq!(meds[0]["frequency"], Value::Null);
        assert!(!note.to_string().contains("null\""), "{note}");
    }
}
