//! Recorded fingerprints of the built-in templates (ADR-0024 section 9).
//!
//! `packages/templates/fingerprints.json` holds, for every built-in, its
//! version, its [`fingerprint`](crate::template::fingerprint) and the SHA-256
//! of the system prompt it produces. Tests fail when:
//! - a built-in's content changes but its version doesn't (people with their
//!   own version are told about a newer original only when the version moves);
//! - the prompt of any built-in changes without the file being rewritten, so a
//!   change to what the model is told is always deliberate (and re-evaluated).
//!
//! `openbark templates fingerprints --write` rewrites the file, and the full
//! SOAP prompt in `packages/templates/soap/prompt.txt`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::template::{LoadedTemplate, fingerprint};

/// The file as shipped.
pub const RECORDED: &str = include_str!("../../../packages/templates/fingerprints.json");

/// The SOAP note's whole system prompt, as shipped (a readable golden file).
pub const SOAP_PROMPT: &str = include_str!("../../../packages/templates/soap/prompt.txt");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub version: String,
    pub fingerprint: String,
    /// SHA-256 of the system prompt.
    pub prompt: String,
}

/// What every shipped built-in is now.
pub fn current() -> BTreeMap<String, Recorded> {
    LoadedTemplate::builtin_ids()
        .map(|id| {
            let t = LoadedTemplate::builtin(id).expect("built-in");
            let prompt = crate::note::system_prompt(&t);
            let rec = Recorded {
                version: t.template.version.clone(),
                fingerprint: fingerprint(&t.template),
                prompt: hex::encode(Sha256::digest(prompt.as_bytes())),
            };
            (id.to_string(), rec)
        })
        .collect()
}

/// The fingerprints file's text for [`current`].
pub fn to_json(map: &BTreeMap<String, Recorded>) -> String {
    let mut text = serde_json::to_string_pretty(map).expect("fingerprints serialise");
    text.push('\n');
    text
}

/// The recorded fingerprints (an empty map if the file can't be read).
pub fn recorded() -> BTreeMap<String, Recorded> {
    serde_json::from_str(RECORDED).unwrap_or_default()
}

/// Why the recorded file and the shipped built-ins disagree, one line each.
pub fn problems() -> Vec<String> {
    let recorded = recorded();
    let mut out = Vec::new();
    for (id, now) in current() {
        let Some(was) = recorded.get(&id) else {
            out.push(format!(
                "{id} isn't in fingerprints.json. Run `openbark templates fingerprints --write`."
            ));
            continue;
        };
        if now.fingerprint != was.fingerprint {
            if now.version == was.version {
                out.push(format!(
                    "{id} changed without a version bump. Bump its version, then run `openbark templates fingerprints --write`."
                ));
            } else {
                out.push(format!(
                    "{id} moved to version {} but fingerprints.json still has {}. Run `openbark templates fingerprints --write`.",
                    now.version, was.version
                ));
            }
        } else if now.version != was.version {
            out.push(format!(
                "{id}'s version changed but its content didn't. Run `openbark templates fingerprints --write`."
            ));
        }
        if now.prompt != was.prompt && now.fingerprint == was.fingerprint {
            out.push(format!(
                "The prompt of {id} changed. If that's deliberate, re-run the eval and then `openbark templates fingerprints --write`."
            ));
        }
    }
    for id in recorded.keys() {
        if !LoadedTemplate::is_builtin(id) {
            out.push(format!("{id} is no longer built in. Run `openbark templates fingerprints --write`."));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A built-in can't change without a version bump, and what the model is
    /// told can't change without the file being rewritten (ADR-0024 section 9).
    #[test]
    fn builtin_fingerprints_match_the_recorded_file() {
        let problems = problems();
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    #[test]
    fn the_soap_prompt_matches_its_golden_file() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let prompt = crate::note::system_prompt(&soap);
        assert_eq!(
            prompt.replace("\r\n", "\n"),
            SOAP_PROMPT.replace("\r\n", "\n"),
            "The SOAP prompt changed. If that's deliberate, run `openbark templates fingerprints --write`."
        );
    }

    #[test]
    fn a_one_word_change_without_a_bump_is_caught() {
        let mut soap = LoadedTemplate::builtin("soap").unwrap().template;
        let before = fingerprint(&soap);
        soap.sections[0].instruction = Some("Changed.".into());
        assert_ne!(fingerprint(&soap), before);
        // Standards, origin, id and version don't count.
        let mut same = LoadedTemplate::builtin("soap").unwrap().template;
        same.id = "other".into();
        same.version = "9".into();
        same.standards.push(crate::template::Standard {
            id: "s".into(),
            heading: String::new(),
            text: "x".into(),
            added_at: chrono::Local::now(),
            added_by: None,
            scope: crate::template::Scope::Team,
        });
        same.origin = Some(crate::template::Origin {
            layer: crate::template::Layer::Team,
            owner: None,
            version: "9".into(),
            fingerprint: String::new(),
            based_on: None,
            seen_original: None,
            saved_by: None,
            approved_by: None,
            saved_at: None,
            object_id: None,
        });
        assert_eq!(fingerprint(&same), before);
    }
}
