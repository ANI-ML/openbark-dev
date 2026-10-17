//! Note templates: YAML instructions + a JSON Schema for the structured core.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Required,
    Optional,
    IfDiscussed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Statements,
    Fields,
    Medications,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Part {
    pub key: String,
    pub label: String,
    pub kind: PartKind,
    pub presence: Presence,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub presence: Presence,
    /// A section is either a flat set of fields (`kind: fields`) or has parts.
    #[serde(default)]
    pub kind: Option<PartKind>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Template {
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub schema: String,
    #[serde(default = "default_language")]
    pub language: String,
    pub instructions: String,
    #[serde(default)]
    pub rules: Vec<String>,
    pub sections: Vec<Section>,
}

fn default_language() -> String {
    "en".into()
}

#[derive(Debug, Clone)]
pub struct LoadedTemplate {
    pub template: Template,
    pub schema: serde_json::Value,
    pub schema_text: String,
}

const BUILTIN: &[(&str, &str, &str)] = &[(
    "soap",
    include_str!("../../../packages/templates/soap/template.yaml"),
    include_str!("../../../packages/templates/soap/schema.json"),
)];

impl LoadedTemplate {
    pub fn builtin_ids() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|(id, _, _)| *id)
    }

    pub fn builtin(id: &str) -> Option<Self> {
        let (_, yaml, schema) = BUILTIN.iter().find(|(i, _, _)| *i == id)?;
        Some(Self::parse(yaml, schema).expect("built-in templates are valid"))
    }

    /// Load `<dir>/template.yaml` and the schema file it names.
    pub fn from_dir(dir: &Path) -> Result<Self, EngineError> {
        let read = |p: &Path| {
            std::fs::read_to_string(p).map_err(|e| EngineError::Template(format!("{}: {e}", p.display())))
        };
        let yaml = read(&dir.join("template.yaml"))?;
        let template: Template =
            serde_yaml_ng::from_str(&yaml).map_err(|e| EngineError::Template(e.to_string()))?;
        let schema = read(&dir.join(&template.schema))?;
        Self::parse(&yaml, &schema)
    }

    /// Look up a built-in template by id, or treat `spec` as a directory.
    pub fn resolve(spec: &str) -> Result<Self, EngineError> {
        if let Some(t) = Self::builtin(spec) {
            return Ok(t);
        }
        let path = Path::new(spec);
        if path.is_dir() {
            return Self::from_dir(path);
        }
        Err(EngineError::Template(format!(
            "unknown template '{spec}' (built-in: {})",
            Self::builtin_ids().collect::<Vec<_>>().join(", ")
        )))
    }

    fn parse(yaml: &str, schema_text: &str) -> Result<Self, EngineError> {
        let template: Template =
            serde_yaml_ng::from_str(yaml).map_err(|e| EngineError::Template(e.to_string()))?;
        let schema: serde_json::Value =
            serde_json::from_str(schema_text).map_err(|e| EngineError::Template(e.to_string()))?;
        jsonschema::validator_for(&schema).map_err(|e| EngineError::Template(format!("schema: {e}")))?;
        let loaded = Self { template, schema, schema_text: schema_text.to_string() };
        loaded.check_sections_match_schema()?;
        Ok(loaded)
    }

    /// Every section/part/field the template renders must exist in the schema.
    fn check_sections_match_schema(&self) -> Result<(), EngineError> {
        let props = |v: &serde_json::Value| v.get("properties").cloned().unwrap_or_default();
        let root = props(&self.schema);
        for section in &self.template.sections {
            let sec = root
                .get(&section.id)
                .ok_or_else(|| EngineError::Template(format!("section '{}' not in schema", section.id)))?;
            for field in &section.fields {
                if props(sec).get(&field.key).is_none() {
                    return Err(EngineError::Template(format!(
                        "field '{}.{}' not in schema",
                        section.id, field.key
                    )));
                }
            }
            for part in &section.parts {
                let p = props(sec).get(&part.key).cloned().ok_or_else(|| {
                    EngineError::Template(format!("part '{}.{}' not in schema", section.id, part.key))
                })?;
                for field in &part.fields {
                    if props(&p).get(&field.key).is_none() {
                        return Err(EngineError::Template(format!(
                            "field '{}.{}.{}' not in schema",
                            section.id, part.key, field.key
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        format!("{}@{}", self.template.id, self.template.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_soap_loads_and_matches_schema() {
        let t = LoadedTemplate::builtin("soap").unwrap();
        assert_eq!(t.template.id, "soap");
        assert_eq!(t.label(), "soap@0.1.0");
        assert!(t.template.sections.iter().any(|s| s.id == "plan"));
        assert!(t.template.rules.iter().any(|r| r.contains("Do not add")));
    }

    #[test]
    fn mismatched_section_is_rejected() {
        let yaml = "id: x\nversion: '1'\nname: X\nschema: s.json\ninstructions: hi\nsections:\n  - {id: nope, title: Nope, presence: required}\n";
        let schema = r#"{"type":"object","properties":{"yes":{"type":"object"}}}"#;
        let err = LoadedTemplate::parse(yaml, schema).unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn unknown_template_lists_builtins() {
        let err = LoadedTemplate::resolve("does-not-exist").unwrap_err();
        assert!(err.to_string().contains("soap"));
    }
}
