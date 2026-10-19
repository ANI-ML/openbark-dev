//! `openbark templates`: list, show and check note templates.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Subcommand;
use os_engine::LoadedTemplate;
use os_engine::template_store::{TemplateStore, Viewer};
use os_engine::users::{TemplateMode, UserStore, Who};

#[derive(Subcommand)]
pub enum TemplatesCommand {
    /// Built-in and team templates.
    List {
        /// Resolve as this team member (id or name): their own versions first.
        #[arg(long = "as")]
        member: Option<String>,
    },
    /// Print a template's headings and the prompt it produces.
    Show {
        /// Template id or a template directory.
        template: String,
        /// Print the generated JSON Schema instead.
        #[arg(long)]
        schema: bool,
        /// Resolve as this team member (default: the shipped built-ins and the folder).
        #[arg(long = "as")]
        member: Option<String>,
    },
    /// What changed between a version and its original (ADR-0024 section 9).
    Diff {
        template: String,
        #[arg(long = "as")]
        member: Option<String>,
        /// `original` (the original now against the version) or `base` (the
        /// original when the version was made against the original now).
        #[arg(long, default_value = "original")]
        with: String,
    },
    /// Check a template.yaml (or its directory) before sharing it.
    Check { path: PathBuf },
    /// Print the built-in templates' fingerprints (ADR-0024 section 9).
    Fingerprints {
        /// Rewrite packages/templates/fingerprints.json and the SOAP prompt file.
        #[arg(long)]
        write: bool,
        /// The built-in templates folder (run from the repository root).
        #[arg(long, default_value = "packages/templates")]
        dir: PathBuf,
    },
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

/// Resolve as a team member, from the team file (`OPENBARK_USERS_FILE`).
fn viewer_as(member: Option<&str>) -> Result<Viewer> {
    let users = UserStore::default();
    let profiles = users.path().parent().map(|p| p.join("profiles")).unwrap_or_default();
    let Some(member) = member else { return Ok(Viewer { profiles, ..Viewer::alone() }) };
    let user = users
        .list()?
        .into_iter()
        .find(|u| u.id == member || u.name.eq_ignore_ascii_case(member))
        .ok_or_else(|| anyhow::anyhow!("No team member called {member}."))?;
    let personal = users.settings().templates == TemplateMode::Personal;
    Ok(Viewer::member(Who::from(&user), profiles, personal))
}

pub fn run(cmd: TemplatesCommand, models: &os_models::ModelStore) -> Result<()> {
    let store = TemplateStore::default();
    match cmd {
        TemplatesCommand::List { member } => {
            let viewer = viewer_as(member.as_deref())?;
            for t in store.list_for(&viewer) {
                let layer = match t.layer {
                    os_engine::template::Layer::Builtin => "built-in",
                    os_engine::template::Layer::Team if t.builtin => "team's version",
                    os_engine::template::Layer::Team => "team",
                    os_engine::template::Layer::Personal if t.edited => "own version",
                    os_engine::template::Layer::Personal => "only them",
                };
                println!(
                    "{:<24} {:<8} {:<14} {:>3} headings  {:>2} standards  {}{}",
                    t.id,
                    t.version,
                    layer,
                    t.headings,
                    t.standards,
                    t.name,
                    if t.original_changed.is_some() { "  (newer original)" } else { "" }
                );
            }
            eprintln!("team templates: {}", store.root().display());
        }
        TemplatesCommand::Diff { template, member, with } => {
            let viewer = viewer_as(member.as_deref())?;
            if with != "original" && with != "base" {
                bail!("--with is original or base");
            }
            let diff = store.compare(&viewer, &template, with == "base")?;
            if diff.is_empty() {
                println!("No differences.");
            } else {
                println!("{}", serde_json::to_string_pretty(&diff)?);
            }
        }
        TemplatesCommand::Show { template, schema, member: Some(member) } => {
            let t = store.resolve_for(&template, &viewer_as(Some(&member))?)?.loaded;
            if schema {
                println!("{}", serde_json::to_string_pretty(&t.schema)?);
            } else {
                println!("{}", os_engine::note::system_prompt(&t));
            }
        }
        TemplatesCommand::Show { template, schema, member: None } => {
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
            // Warnings never block: the rules that always apply win (ADR-0024 section 4.4).
            for w in os_engine::template_lint::lint_template(&t.template) {
                println!(
                    "warning: {}: \"{}\" conflicts with the rules that always apply ({:?})",
                    w.path, w.text, w.reason
                );
            }
        }
        TemplatesCommand::Fingerprints { write, dir } => {
            let now = os_engine::fingerprints::current();
            let json = os_engine::fingerprints::to_json(&now);
            if write {
                std::fs::write(dir.join("fingerprints.json"), &json)?;
                let soap = LoadedTemplate::builtin("soap").expect("SOAP is built in");
                std::fs::write(dir.join("soap").join("prompt.txt"), os_engine::note::system_prompt(&soap))?;
                eprintln!("wrote {} and the SOAP prompt", dir.join("fingerprints.json").display());
            } else {
                print!("{json}");
                for p in os_engine::fingerprints::problems() {
                    eprintln!("{p}");
                }
            }
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
