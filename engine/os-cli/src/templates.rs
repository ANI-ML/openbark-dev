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
}

pub fn run(cmd: TemplatesCommand) -> Result<()> {
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
    }
    Ok(())
}
