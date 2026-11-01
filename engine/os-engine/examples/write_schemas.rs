//! Write every JSON Schema OpenBark drafts against into a folder, for the
//! golden check that they all compile in llguidance (ADR-0025 decision 3):
//! each built-in template's note schema, its edit schema, the
//! template-from-example schema, and the drafting schema of a heading with
//! normal findings (ADR-0032), whose statements name their system.
//!
//! ```text
//! cargo run -p os-engine --example write_schemas -- <folder>
//! OPENBARK_MLX_TEST_SCHEMAS=<folder> cargo test -p os-llm --test mlx_worker golden
//! ```

use std::path::PathBuf;

use os_engine::LoadedTemplate;

/// A French exam heading with normal findings, accents in its systems.
const NORMAL_FINDINGS: &str = r#"
name: Examen
language: fr
sections:
  - title: Examen physique
    kind: statements
    defaults:
      - { label: Yeux, text: "Normal, aucun écoulement" }
      - { label: Cœur, text: "Rythme régulier" }
      - { label: Nœuds lymphatiques, text: "Normaux" }
"#;

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "target/schemas".into()));
    std::fs::create_dir_all(&out)?;
    let mut written = 0;
    let mut write = |name: String, schema: &serde_json::Value| -> std::io::Result<()> {
        std::fs::write(out.join(name), serde_json::to_string_pretty(schema)?)?;
        written += 1;
        Ok(())
    };
    for id in LoadedTemplate::builtin_ids() {
        let t = LoadedTemplate::builtin(id).expect("built-in templates load");
        write(format!("note_{id}.json"), &t.schema)?;
        write(format!("edit_{id}.json"), &os_engine::edit::editable_schema(&t))?;
    }
    write("from_example.json".into(), &os_engine::from_example::draft_schema())?;
    let exam = LoadedTemplate::from_yaml(NORMAL_FINDINGS).expect("the sample template loads");
    let drafting: serde_json::Value = serde_json::from_str(&os_engine::note::drafting_schema_text(&exam))?;
    write("draft_normal_findings.json".into(), &drafting)?;
    eprintln!("{written} schemas written to {}", out.display());
    Ok(())
}
