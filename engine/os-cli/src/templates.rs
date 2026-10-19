//! `openbark templates`: list, show and check note templates.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use os_engine::LoadedTemplate;
use os_engine::template_store::TemplateStore;

#[derive(Subcommand)]
pub enum TemplatesCommand {
    /// Built-in and team templates.
    List,
    /// Print a template's headings and the prompt it produces.
    Show {
        /// Template id or a template directory.
        template: String,
        /// Print the generated JSON Schema instead.
        #[arg(long)]
        schema: bool,
    },
    /// Check a template.yaml (or its directory) before sharing it.
    Check { path: PathBuf },
    /// Propose a template from an example note (a text file); prints YAML.
    FromExample {
        file: PathBuf,
        /// Expected headings, one per line: report which were found.
        #[arg(long)]
        expect: Option<PathBuf>,
        #[command(flatten)]
        llm: crate::LlmArgs,
    },
}

pub fn run(cmd: TemplatesCommand, models: &os_models::ModelStore) -> Result<()> {
    let store = TemplateStore::default();
    match cmd {
        TemplatesCommand::List => {
            for t in store.list() {
                println!(
                    "{:<24} {:<8} {:<9} {:>3} headings  {:>2} standards  {}",
                    t.id,
                    t.version,
                    if t.builtin { "built-in" } else { "team" },
                    t.headings,
                    t.standards,
                    t.name
                );
            }
            eprintln!("team templates: {}", store.root().display());
        }
        TemplatesCommand::Show { template, schema } => {
            let t = LoadedTemplate::resolve(&template)?;
            if schema {
                println!("{}", serde_json::to_string_pretty(&t.schema)?);
            } else {
                println!("{}", os_engine::note::system_prompt(&t));
            }
        }
        TemplatesCommand::Check { path } => {
            let t = if path.is_dir() {
                LoadedTemplate::from_dir(&path)?
            } else {
                LoadedTemplate::from_yaml(&std::fs::read_to_string(&path)?)?
            };
            let headings: usize = t.template.sections.iter().map(|s| 1 + s.parts.len()).sum();
            println!("{} ({}): {headings} headings, ready to use", t.template.name, t.label());
        }
        TemplatesCommand::FromExample { file, expect, llm } => {
            let backend = crate::load_llm(models, &llm)?;
            let t =
                os_engine::from_example::template_from_example(&*backend, &std::fs::read_to_string(&file)?)?;
            println!("{}", serde_yaml_ng::to_string(&t.template)?);
            if let Some(expect) = expect {
                let got: Vec<String> = t
                    .template
                    .sections
                    .iter()
                    .flat_map(|s| {
                        std::iter::once(s.title.clone()).chain(s.parts.iter().map(|p| p.label.clone()))
                    })
                    .map(|h| h.to_lowercase())
                    .collect();
                let wanted: Vec<String> = std::fs::read_to_string(&expect)?
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                // A line may list alternatives: "S | Subjective".
                let found = |w: &String| w.split('|').any(|alt| got.contains(&alt.trim().to_lowercase()));
                let missing: Vec<&String> = wanted.iter().filter(|w| !found(w)).collect();
                eprintln!(
                    "{}: {}/{} expected headings found{}",
                    file.display(),
                    wanted.len() - missing.len(),
                    wanted.len(),
                    if missing.is_empty() { String::new() } else { format!("; missing: {missing:?}") }
                );
            }
        }
    }
    Ok(())
}
