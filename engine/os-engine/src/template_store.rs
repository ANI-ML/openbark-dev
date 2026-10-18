//! Team templates and saved standards, kept as plain files so a clinic can
//! share them or keep them under version control.
//!
//! ```text
//! <templates root>/
//!   dental-procedure/template.yaml   a team template (headings + instructions)
//!   dental-procedure/standards.yaml  re-prompts saved as standards
//!   soap/standards.yaml              standards for a built-in template
//! ```
//!
//! The root is `OPENBARK_TEMPLATES_DIR`, else `<local data>/OpenBark/templates`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::Serialize;

use crate::EngineError;
use crate::template::{LoadedTemplate, Standard, Template};

const TEMPLATE_FILE: &str = "template.yaml";
const STANDARDS_FILE: &str = "standards.yaml";
const MAX_STANDARD: usize = 500;

pub fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPENBARK_TEMPLATES_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark").join("templates")
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub builtin: bool,
    /// "record" or "client".
    pub document: crate::template::Document,
    /// Icon for this kind of visit (a Phosphor name; "" for the default).
    pub icon: String,
    pub headings: usize,
    pub standards: usize,
}

#[derive(Debug, Clone)]
pub struct TemplateStore {
    root: PathBuf,
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self::new(default_root())
    }
}

impl TemplateStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Built-in templates first, then team templates by name.
    pub fn list(&self) -> Vec<TemplateSummary> {
        let mut out: Vec<TemplateSummary> = LoadedTemplate::builtin_ids()
            .filter_map(|id| self.load(id).ok())
            .map(|t| summary(&t, true))
            .collect();
        let mut team: Vec<TemplateSummary> = fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join(TEMPLATE_FILE).is_file())
            .filter_map(|e| self.load(&e.file_name().to_string_lossy()).ok())
            .filter(|t| !LoadedTemplate::is_builtin(&t.template.id))
            .map(|t| summary(&t, false))
            .collect();
        team.sort_by_key(|t| t.name.to_lowercase());
        out.extend(team);
        out
    }

    /// A built-in or team template, with its standards merged in.
    pub fn load(&self, id: &str) -> Result<LoadedTemplate, EngineError> {
        let mut t = match LoadedTemplate::builtin(id) {
            Some(t) => t,
            None => {
                let dir = self.dir(id)?;
                if !dir.join(TEMPLATE_FILE).is_file() {
                    return Err(EngineError::Template(format!(
                        "unknown template '{id}' (built-in: {})",
                        LoadedTemplate::builtin_ids().collect::<Vec<_>>().join(", ")
                    )));
                }
                let mut t = LoadedTemplate::from_dir(&dir)?;
                // The folder name is the id, whatever the file says.
                t.template.id = id.to_string();
                t
            }
        };
        t.template.standards = self.standards(id)?;
        Ok(t)
    }

    /// Save a team template. Its version goes up by one when it changes.
    /// Built-in templates can't be overwritten; duplicate them instead.
    pub fn save(&self, mut template: Template) -> Result<LoadedTemplate, EngineError> {
        template.standards.clear(); // kept in standards.yaml
        template.schema = None; // team templates always use the generated schema
        if template.id.trim().is_empty() {
            // A new template: never take over an existing one's id.
            let base = crate::template::id_from(&template.name);
            let mut id = base.clone();
            let mut n = 2;
            while LoadedTemplate::is_builtin(&id) || self.root.join(&id).join(TEMPLATE_FILE).is_file() {
                id = format!("{base}-{n}");
                n += 1;
            }
            template.id = id;
        }
        let mut loaded = LoadedTemplate::new(template)?;
        let id = loaded.template.id.clone();
        if LoadedTemplate::is_builtin(&id) {
            return Err(EngineError::Template(format!(
                "'{id}' is a built-in template and can't be changed. Duplicate it to make your own."
            )));
        }
        let dir = self.dir(&id)?;
        let path = dir.join(TEMPLATE_FILE);
        let previous = LoadedTemplate::from_dir(&dir).ok();
        loaded.template.version = match &previous {
            None => "1".into(),
            Some(prev) => {
                let mut same = prev.template.clone();
                same.version = loaded.template.version.clone();
                if serde_json::to_value(&same).ok() == serde_json::to_value(&loaded.template).ok() {
                    prev.template.version.clone()
                } else {
                    next_version(&prev.template.version)
                }
            }
        };
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        write_atomic(&path, &loaded.to_yaml())?;
        loaded.template.standards = self.standards(&id)?;
        Ok(loaded)
    }

    /// Delete a team template and its standards.
    pub fn delete(&self, id: &str) -> Result<(), EngineError> {
        if LoadedTemplate::is_builtin(id) {
            return Err(EngineError::Template(format!(
                "'{id}' is a built-in template and can't be deleted."
            )));
        }
        let dir = self.dir(id)?;
        if !dir.join(TEMPLATE_FILE).is_file() {
            return Err(EngineError::Template(format!("No team template called {id}.")));
        }
        fs::remove_dir_all(&dir).map_err(io(&dir))
    }

    pub fn standards(&self, id: &str) -> Result<Vec<Standard>, EngineError> {
        let path = self.dir(id)?.join(STANDARDS_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => serde_yaml_ng::from_str(&text)
                .map_err(|e| EngineError::Template(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(io(&path)(e)),
        }
    }

    /// Save a re-prompt as a standard for a heading ("" = the whole note).
    pub fn add_standard(
        &self,
        id: &str,
        heading: &str,
        text: &str,
        by: Option<&str>,
    ) -> Result<Standard, EngineError> {
        let template = self.load(id)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(EngineError::Template("Write the standard first.".into()));
        }
        if text.chars().count() > MAX_STANDARD {
            return Err(EngineError::Template(format!(
                "A standard can be at most {MAX_STANDARD} characters."
            )));
        }
        if !heading.is_empty() && template.heading_title(heading).is_none() {
            return Err(EngineError::Template(format!(
                "{} has no heading '{heading}'.",
                template.template.name
            )));
        }
        let mut all = template.template.standards;
        if let Some(existing) = all.iter().find(|s| s.heading == heading && s.text.eq_ignore_ascii_case(text))
        {
            return Ok(existing.clone());
        }
        let standard = Standard {
            id: uuid::Uuid::now_v7().to_string(),
            heading: heading.to_string(),
            text: text.to_string(),
            added_at: Local::now(),
            added_by: by.map(str::trim).filter(|b| !b.is_empty()).map(String::from),
        };
        all.push(standard.clone());
        self.write_standards(id, &all)?;
        Ok(standard)
    }

    pub fn remove_standard(&self, id: &str, standard_id: &str) -> Result<(), EngineError> {
        let mut all = self.standards(id)?;
        let before = all.len();
        all.retain(|s| s.id != standard_id);
        if all.len() == before {
            return Err(EngineError::Template("That standard was already removed.".into()));
        }
        self.write_standards(id, &all)
    }

    fn write_standards(&self, id: &str, all: &[Standard]) -> Result<(), EngineError> {
        let dir = self.dir(id)?;
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        let text = serde_yaml_ng::to_string(all).expect("standards serialise");
        write_atomic(&dir.join(STANDARDS_FILE), &text)
    }

    /// A template's folder, refusing ids that could escape the root.
    fn dir(&self, id: &str) -> Result<PathBuf, EngineError> {
        let ok = !id.is_empty()
            && id.len() <= 48
            && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok {
            return Err(EngineError::Template(format!("'{id}' is not a template id.")));
        }
        Ok(self.root.join(id))
    }
}

fn summary(t: &LoadedTemplate, builtin: bool) -> TemplateSummary {
    let headings = t.template.sections.iter().map(|s| 1 + s.parts.len()).sum();
    TemplateSummary {
        id: t.template.id.clone(),
        name: t.template.name.clone(),
        description: t.template.description.clone(),
        icon: t.template.icon.clone(),
        version: t.template.version.clone(),
        builtin,
        document: t.template.document,
        headings,
        standards: t.template.standards.len(),
    }
}

/// "1" → "2"; "0.1.0" → "0.1.1"; anything else gets ".1".
fn next_version(v: &str) -> String {
    if let Ok(n) = v.parse::<u64>() {
        return (n + 1).to_string();
    }
    match v.rsplit_once('.').map(|(head, last)| (head, last.parse::<u64>())) {
        Some((head, Ok(n))) => format!("{head}.{}", n + 1),
        _ => format!("{v}.1"),
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), EngineError> {
    let tmp = path.with_extension("yaml.tmp");
    fs::write(&tmp, text).map_err(io(&tmp))?;
    fs::rename(&tmp, path).map_err(io(path))
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> EngineError + '_ {
    move |e| EngineError::Template(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dental() -> Template {
        serde_yaml_ng::from_str(
            "name: Dental procedure\nsections:\n  - { title: Anaesthesia, kind: medications }\n  - { title: Findings, kind: statements }\n",
        )
        .unwrap()
    }

    #[test]
    fn versions() {
        assert_eq!(next_version("1"), "2");
        assert_eq!(next_version("0.1.0"), "0.1.1");
        assert_eq!(next_version("beta"), "beta.1");
    }

    #[test]
    fn save_list_version_and_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TemplateStore::new(tmp.path().into());
        let builtins = LoadedTemplate::builtin_ids().count();
        assert_eq!(store.list().len(), builtins, "only the built-ins at first");

        let saved = store.save(dental()).unwrap();
        assert_eq!(saved.label(), "dental-procedure@1");
        // Saving the same thing again keeps the version.
        assert_eq!(store.save(saved.template.clone()).unwrap().template.version, "1");
        // A change bumps it.
        let mut changed = saved.template.clone();
        changed.sections[1].instruction = Some("By quadrant.".into());
        assert_eq!(store.save(changed).unwrap().template.version, "2");

        let list = store.list();
        assert_eq!(list.len(), builtins + 1);
        assert!(list[0].builtin && !list[builtins].builtin);
        assert_eq!(list[builtins].headings, 2);
        assert_eq!(
            LoadedTemplate::from_dir(&tmp.path().join("dental-procedure")).unwrap().template.version,
            "2"
        );

        // A new template with the same name gets its own id.
        assert_eq!(store.save(dental()).unwrap().template.id, "dental-procedure-2");
        let mut named_soap = dental();
        named_soap.name = "SOAP".into();
        assert_eq!(store.save(named_soap).unwrap().template.id, "soap-2");

        store.delete("dental-procedure").unwrap();
        assert_eq!(store.list().len(), builtins + 2);
    }

    #[test]
    fn builtins_are_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TemplateStore::new(tmp.path().into());
        let soap = store.load("soap").unwrap().template;
        assert!(store.save(soap).unwrap_err().to_string().contains("Duplicate"));
        assert!(store.delete("soap").is_err());
        assert!(store.load("../etc").is_err());
    }

    #[test]
    fn standards_for_builtin_and_team_templates() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TemplateStore::new(tmp.path().into());
        let s = store
            .add_standard(
                "soap",
                "plan/medications",
                "List every fluid with its rate and volume",
                Some("Dr Patel"),
            )
            .unwrap();
        // The same request again is not duplicated.
        let again = store
            .add_standard("soap", "plan/medications", "list every fluid with its rate and volume", None)
            .unwrap();
        assert_eq!(again.id, s.id);
        assert!(store.add_standard("soap", "plan/nope", "x", None).is_err());
        store.add_standard("soap", "", "Use metric units throughout", None).unwrap();

        let soap = store.load("soap").unwrap();
        assert_eq!(soap.template.standards.len(), 2);
        assert_eq!(
            soap.standards_for("plan/medications").next().unwrap().added_by.as_deref(),
            Some("Dr Patel")
        );
        assert_eq!(store.list().iter().find(|t| t.id == "soap").unwrap().standards, 2);
        // A soap folder holding only standards is not listed as a team template.
        assert_eq!(store.list().len(), LoadedTemplate::builtin_ids().count());

        store.remove_standard("soap", &s.id).unwrap();
        assert_eq!(store.load("soap").unwrap().template.standards.len(), 1);
        assert!(store.remove_standard("soap", &s.id).is_err());
    }
}
