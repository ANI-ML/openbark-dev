//! Templates in three layers (ADR-0024 section 2), and the standards saved
//! against them.
//!
//! ```text
//! <templates root>/                          the team layer: plain YAML a clinic can share
//!   dental-procedure/template.yaml           a team template
//!   dental-procedure/standards.yaml          re-prompts saved as standards for everyone
//!   soap/template.yaml                       the team's version of the built-in SOAP note
//!   soap/base.yaml                           the built-in as it was when that version was made
//!   soap/history/4.yaml                      earlier versions, the last 20
//!
//! <folder of the team file>/profiles/        the personal layer: encrypted (ADR-0008)
//!   <member id>/templates/soap/template.yaml one person's own version
//!   <member id>/templates/soap/base.yaml     the original when they made it
//!   <member id>/templates/soap/standards.yaml their own standards, and team standards they muted
//!   <member id>/templates/soap/history/2.yaml their earlier versions, the last 20
//!   <member id>/templates/my-dental/...      a template only they use
//! ```
//!
//! The person drafting gets their own version first, then the team's, then
//! the built-in ([`TemplateStore::resolve_for`]). The command line and the
//! eval keep drafting with the shipped built-ins ([`LoadedTemplate::resolve`]).
//! Personal files are written as new files, never renamed, because the vault
//! binds each file's name into its encryption. In a team the app calls the
//! team layer "the hospital's".
//!
//! The team root is `OPENBARK_TEMPLATES_DIR`, else `<local data>/OpenBark/templates`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::EngineError;
use crate::template::{BasedOn, Layer, LoadedTemplate, Origin, Scope, Standard, Template, fingerprint};
use crate::template_diff::{TemplateDiff, template_diff};
use crate::users::Who;

const TEMPLATE_FILE: &str = "template.yaml";
const BASE_FILE: &str = "base.yaml";
const STANDARDS_FILE: &str = "standards.yaml";
const HISTORY_DIR: &str = "history";
/// Earlier versions kept per template, per layer.
pub const MAX_HISTORY: usize = 20;
const MAX_STANDARD: usize = 500;

pub const CONFLICT: &str =
    "This template changed while you were editing. Your changes are still here. Compare, then save again.";
pub const DOCUMENT_LOCKED: &str =
    "Who it's for is fixed for this visit type. Duplicate the template to make one for the client.";
pub const FIXED_RULES_LOCKED: &str = "Fixed rules can't be changed. They come from the original template.";
pub const NOTHING_TO_RESET: &str = "You're already using the original.";

pub fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPENBARK_TEMPLATES_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark").join("templates")
}

/// A template as listed: what it is and where it comes from.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// This id is a built-in (grouping on Record and Templates).
    pub builtin: bool,
    /// "record" or "client".
    pub document: crate::template::Document,
    /// Icon for this kind of visit ("" for the default).
    pub icon: String,
    pub headings: usize,
    pub standards: usize,
    /// The layer the person listing gets.
    pub layer: Layer,
    /// It changes an original (a built-in, or the team's template).
    pub edited: bool,
    /// Built in or the team's, as opposed to a template only this person uses.
    pub shared: bool,
    /// The original changed since this version was made or kept.
    pub original_changed: Option<OriginalChanged>,
}

/// What a version is the version of, as it is now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OriginalInfo {
    pub layer: Layer,
    pub id: String,
    pub name: String,
    pub version: String,
    pub fingerprint: String,
}

/// How the original changed (ADR-0024 section 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginalChange {
    /// A release changed the built-in.
    NewerBuiltin,
    /// The team made or changed its version.
    TeamChanged,
    /// The team reset its version to the built-in.
    TeamReset,
    /// The team template it came from was deleted.
    TeamDeleted,
    /// A release removed the built-in.
    NoLongerBuiltin,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OriginalChanged {
    pub reason: OriginalChange,
    /// The original's name and version now (its last name, when it's gone).
    pub name: String,
    pub version: String,
    /// The original's fingerprint now, for "Keep mine" (empty when it's gone).
    pub fingerprint: String,
}

/// Who is looking: a person resolves their own version first.
#[derive(Debug, Clone)]
pub struct Viewer {
    /// The team member, or None when working alone (no team set up).
    pub member: Option<Who>,
    /// `<folder of the team file>/profiles`.
    pub profiles: PathBuf,
    /// Personal versions apply: a team is set up and templates are each
    /// person's own (not shared by the hospital).
    pub personal: bool,
}

impl Viewer {
    /// Working alone: the templates folder is yours.
    pub fn alone() -> Self {
        Self { member: None, profiles: PathBuf::new(), personal: false }
    }

    pub fn member(who: Who, profiles: PathBuf, personal: bool) -> Self {
        Self { member: Some(who), profiles, personal }
    }

    fn member_id(&self) -> Option<&str> {
        self.member.as_ref().and_then(|m| m.id.as_deref()).filter(|id| valid_member(id))
    }

    /// The same person with personal versions off (the team's view).
    fn shared_view(&self) -> Self {
        Self { personal: false, ..self.clone() }
    }

    /// This person's folder for a template id, when personal versions apply.
    fn personal_dir(&self, id: &str) -> Option<Dir> {
        if !self.personal {
            return None;
        }
        let member = self.member_id()?;
        Some(Dir::sealed(profile_templates(&self.profiles, member).join(id)))
    }
}

/// `<profiles>/<member id>/templates`.
fn profile_templates(profiles: &Path, member: &str) -> PathBuf {
    profiles.join(member).join("templates")
}

fn valid_member(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A template as the person gets it, ready to draft with.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The template to draft with: fixed rules from the current built-in,
    /// the standards that apply, and its origin.
    pub loaded: LoadedTemplate,
    pub layer: Layer,
    pub original: Option<OriginalInfo>,
    pub original_changed: Option<OriginalChanged>,
    /// Standards for headings this template doesn't have (not applied).
    pub not_used: Vec<Standard>,
    /// Team standards this person turned off for themselves.
    pub muted: Vec<String>,
    /// A copy of the original from when this version was made is kept.
    pub has_base: bool,
}

impl Resolved {
    pub fn origin(&self) -> &Origin {
        self.loaded.template.origin.as_ref().expect("a resolved template has an origin")
    }
}

/// Undoes a reset: names the history entry the reset moved the version to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoToken {
    pub layer: Layer,
    pub id: String,
    pub entry: String,
}

/// A change to save, where it goes, and who saved and approved it.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub template: Template,
    /// The version the editor started from; a different one is refused.
    pub base_version: Option<String>,
    /// The layer the editor started from.
    pub base_layer: Option<Layer>,
    /// `Team` (working alone, or the hospital's templates) or `Personal`.
    pub target: Layer,
    pub saved_by: Option<Who>,
    pub approved_by: Option<Who>,
}

/// Sharing a personal version with the team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareMode {
    /// It becomes the team's version of that id.
    Replace,
    /// It's added as a new team template with its own id.
    Add,
}

/// What a team member keeps in their profile (the removal confirmation).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProfileSummary {
    /// Their own versions of built-in or team templates.
    pub versions: usize,
    /// Templates only they use.
    pub own_templates: usize,
    pub standards: usize,
}

/// A person's standards file: their own standards, and team standards they muted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersonalStandards {
    #[serde(default)]
    standards: Vec<Standard>,
    #[serde(default)]
    muted: Vec<String>,
}

#[derive(Debug, Clone)]
enum Builtins {
    Shipped,
    /// Another set of built-ins (tests, and the `test-builtins` demo build),
    /// to simulate a release in-process: id and YAML, in listing order.
    Injected(Arc<Vec<(String, String)>>),
}

#[derive(Debug, Clone)]
pub struct TemplateStore {
    root: PathBuf,
    builtins: Builtins,
}

impl Default for TemplateStore {
    fn default() -> Self {
        let store = Self::new(default_root());
        match test_builtins_dir(std::env::var_os("OPENBARK_TEST_BUILTINS_DIR")) {
            Some(dir) => store.with_builtins_from(&dir),
            None => store,
        }
    }
}

/// `OPENBARK_TEST_BUILTINS_DIR`, honoured only by a `test-builtins` build
/// (the end-to-end demo). Release builds ignore it.
fn test_builtins_dir(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if cfg!(feature = "test-builtins") { var.map(PathBuf::from) } else { None }
}

/// A folder holding one template's files, plain (team) or encrypted (personal).
#[derive(Debug, Clone)]
struct Dir {
    path: PathBuf,
    sealed: bool,
}

impl Dir {
    fn plain(path: PathBuf) -> Self {
        Self { path, sealed: false }
    }

    fn sealed(path: PathBuf) -> Self {
        Self { path, sealed: true }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn has(&self, name: &str) -> bool {
        self.file(name).is_file()
    }

    fn read(&self, name: &str) -> Result<Option<String>, EngineError> {
        let path = self.file(name);
        let read = if self.sealed { crate::vault::read_to_string(&path) } else { fs::read_to_string(&path) };
        match read {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&path)(e)),
        }
    }

    fn write(&self, name: &str, text: &str) -> Result<(), EngineError> {
        let path = self.file(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        if self.sealed {
            crate::vault::write(&path, text.as_bytes()).map_err(io(&path))
        } else {
            write_atomic(&path, text)
        }
    }

    fn remove(&self, name: &str) -> Result<(), EngineError> {
        let path = self.file(name);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(&path)(e)),
            _ => Ok(()),
        }
    }

    /// The template stored here, normalised, with the folder name as its id.
    fn load(&self, id: &str, name: &str) -> Result<Option<Template>, EngineError> {
        let Some(text) = self.read(name)? else { return Ok(None) };
        let mut t =
            parse(&text).map_err(|e| EngineError::Template(format!("{}: {e}", self.file(name).display())))?;
        t.id = id.to_string();
        Ok(Some(t))
    }

    /// History entries ("3"), oldest first.
    fn history(&self) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(self.path.join(HISTORY_DIR))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let entry = name.strip_suffix(".yaml")?;
                (!entry.ends_with(".base")).then(|| entry.to_string())
            })
            .collect();
        out.sort_by(|a, b| version_order(a, b));
        out
    }

    /// Keep the last [`MAX_HISTORY`] entries.
    fn prune(&self) -> Result<(), EngineError> {
        let all = self.history();
        for old in all.iter().take(all.len().saturating_sub(MAX_HISTORY)) {
            self.remove(&history_file(old))?;
            self.remove(&history_base(old))?;
        }
        Ok(())
    }

    /// The first version number at this layer: after any kept in history.
    fn first_version(&self) -> String {
        let top = self.history().iter().filter_map(|v| v.parse::<u64>().ok()).max().unwrap_or(0);
        (top + 1).to_string()
    }
}

fn history_file(entry: &str) -> String {
    format!("{HISTORY_DIR}/{entry}.yaml")
}

fn history_base(entry: &str) -> String {
    format!("{HISTORY_DIR}/{entry}.base.yaml")
}

/// A version as a file name: digits, letters, dots and dashes only.
fn entry_name(version: &str) -> String {
    let clean: String = version
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    if clean.is_empty() || clean.starts_with('.') { format!("v{clean}") } else { clean }
}

fn version_order(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    key(a).cmp(&key(b)).then_with(|| a.cmp(b))
}

fn parse(yaml: &str) -> Result<Template, EngineError> {
    let t: Template = serde_yaml_ng::from_str(yaml).map_err(|e| EngineError::Template(e.to_string()))?;
    Ok(LoadedTemplate::new(t)?.template)
}

fn to_yaml(t: &Template) -> String {
    let mut t = t.clone();
    t.standards.clear();
    serde_yaml_ng::to_string(&t).expect("template serialises")
}

impl TemplateStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root, builtins: Builtins::Shipped }
    }

    /// The same store with another set of built-ins (id, YAML), so tests can
    /// simulate a release that adds, changes or removes one.
    pub fn with_builtins(root: PathBuf, builtins: Vec<(String, String)>) -> Self {
        Self { root, builtins: Builtins::Injected(Arc::new(builtins)) }
    }

    /// The shipped built-ins, with any `<dir>/<id>/template.yaml` in place of
    /// (or added to) them: the `test-builtins` demo build.
    fn with_builtins_from(self, dir: &Path) -> Self {
        let mut set: Vec<(String, String)> = LoadedTemplate::builtin_ids()
            .map(|id| (id.to_string(), LoadedTemplate::builtin(id).expect("built-in").to_yaml()))
            .collect();
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if let Ok(yaml) = fs::read_to_string(entry.path().join(TEMPLATE_FILE)) {
                match set.iter_mut().find(|(i, _)| *i == id) {
                    Some(slot) => slot.1 = yaml,
                    None => set.push((id, yaml)),
                }
            }
        }
        Self { root: self.root, builtins: Builtins::Injected(Arc::new(set)) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn builtin_ids(&self) -> Vec<String> {
        match &self.builtins {
            Builtins::Shipped => LoadedTemplate::builtin_ids().map(String::from).collect(),
            Builtins::Injected(set) => set.iter().map(|(id, _)| id.clone()).collect(),
        }
    }

    pub fn is_builtin(&self, id: &str) -> bool {
        match &self.builtins {
            Builtins::Shipped => LoadedTemplate::is_builtin(id),
            Builtins::Injected(set) => set.iter().any(|(i, _)| i == id),
        }
    }

    /// A built-in as this store has it.
    pub fn builtin(&self, id: &str) -> Option<LoadedTemplate> {
        match &self.builtins {
            Builtins::Shipped => LoadedTemplate::builtin(id),
            Builtins::Injected(set) => {
                let (_, yaml) = set.iter().find(|(i, _)| i == id)?;
                let mut t = LoadedTemplate::from_yaml(yaml).ok()?;
                t.template.id = id.to_string();
                t.template.standards.clear();
                t.template.origin = None;
                Some(t)
            }
        }
    }

    fn unknown(&self, id: &str) -> EngineError {
        EngineError::Template(format!(
            "unknown template '{id}' (built-in: {})",
            self.builtin_ids().join(", ")
        ))
    }

    fn team_dir(&self, id: &str) -> Result<Dir, EngineError> {
        Ok(Dir::plain(self.dir(id)?))
    }

    /// Built-in templates first, then team templates by name (no personal layer).
    pub fn list(&self) -> Vec<TemplateSummary> {
        self.list_for(&Viewer::alone())
    }

    /// A team template, the team's version of a built-in, or a built-in, with
    /// the team's standards (no personal layer).
    pub fn load(&self, id: &str) -> Result<LoadedTemplate, EngineError> {
        Ok(self.resolve_for(id, &Viewer::alone())?.loaded)
    }

    // ---- resolution ---------------------------------------------------------------

    /// The template this person gets for an id: their own version, then the
    /// team's, then the built-in (ADR-0024 section 2).
    pub fn resolve_for(&self, id: &str, viewer: &Viewer) -> Result<Resolved, EngineError> {
        let team = self.team_dir(id)?;
        if let Some(dir) = viewer.personal_dir(id)
            && let Some(t) = dir.load(id, TEMPLATE_FILE)?
        {
            return self.resolved(id, viewer, t, Layer::Personal, &dir);
        }
        if let Some(t) = team.load(id, TEMPLATE_FILE)? {
            return self.resolved(id, viewer, t, Layer::Team, &team);
        }
        match self.builtin(id) {
            Some(b) => self.resolved(id, viewer, b.template, Layer::Builtin, &team),
            None => Err(self.unknown(id)),
        }
    }

    /// What a version at `layer` is the version of, as stored now.
    fn original_of(&self, id: &str, layer: Layer) -> Result<Option<(Template, Layer)>, EngineError> {
        match layer {
            Layer::Builtin => Ok(None),
            Layer::Personal => match self.team_dir(id)?.load(id, TEMPLATE_FILE)? {
                Some(t) => Ok(Some((t, Layer::Team))),
                None => Ok(self.builtin(id).map(|b| (b.template, Layer::Builtin))),
            },
            Layer::Team => Ok(self.builtin(id).map(|b| (b.template, Layer::Builtin))),
        }
    }

    /// The fixed rules a template drafts with: the current built-in's, or its
    /// own copy when the id isn't (or is no longer) built in.
    fn fixed_rules_for(&self, id: &str, own: &[String]) -> Vec<String> {
        self.builtin(id).map(|b| b.template.fixed_rules).unwrap_or_else(|| own.to_vec())
    }

    fn resolved(
        &self,
        id: &str,
        viewer: &Viewer,
        stored: Template,
        layer: Layer,
        dir: &Dir,
    ) -> Result<Resolved, EngineError> {
        let original = self.original_of(id, layer)?.map(|(t, l)| OriginalInfo {
            layer: l,
            id: id.to_string(),
            name: t.name.clone(),
            version: t.version.clone(),
            fingerprint: fingerprint(&t),
        });
        let mut origin = match (layer, stored.origin.clone()) {
            (Layer::Builtin, _) | (_, None) => blank_origin(layer, &stored.version),
            (_, Some(o)) => o,
        };
        let mut t = stored;
        t.fixed_rules = self.fixed_rules_for(id, &t.fixed_rules);
        t.standards.clear();
        t.origin = None;
        let mut loaded = LoadedTemplate::new(t)?;
        origin.layer = layer;
        origin.version = loaded.template.version.clone();
        origin.fingerprint = fingerprint(&loaded.template);
        if layer == Layer::Personal && origin.owner.is_none() {
            origin.owner = viewer.member.clone();
        }
        let original_changed = origin.based_on.as_ref().and_then(|b| {
            let based_name =
                || dir.load(id, BASE_FILE).ok().flatten().map(|t| t.name).unwrap_or_else(|| id.to_string());
            changed_original(b, origin.seen_original.as_deref(), original.as_ref(), based_name)
        });
        // Standards: the team's (less any this person muted), then their own.
        let personal = self.personal_standards(viewer, id)?;
        let mut all: Vec<Standard> = self
            .standards(id)?
            .into_iter()
            .filter(|s| !personal.muted.contains(&s.id))
            .map(|s| Standard { scope: Scope::Team, ..s })
            .collect();
        all.extend(personal.standards.into_iter().map(|s| Standard { scope: Scope::Personal, ..s }));
        let (applied, not_used): (Vec<Standard>, Vec<Standard>) =
            all.into_iter().partition(|s| s.heading.is_empty() || loaded.heading_title(&s.heading).is_some());
        loaded.template.standards = applied;
        loaded.template.origin = Some(origin);
        Ok(Resolved {
            loaded,
            layer,
            original,
            original_changed,
            not_used,
            muted: personal.muted,
            has_base: layer != Layer::Builtin && dir.has(BASE_FILE),
        })
    }

    /// Every template this person can draft with: built-ins (with their or the
    /// team's version), team templates by name, then templates only they use.
    pub fn list_for(&self, viewer: &Viewer) -> Vec<TemplateSummary> {
        let mut out: Vec<TemplateSummary> = self
            .builtin_ids()
            .iter()
            .filter_map(|id| self.resolve_for(id, viewer).ok())
            .map(|r| self.summary(&r))
            .collect();
        let team_ids: Vec<String> =
            dirs_with_template(&self.root).into_iter().filter(|id| !self.is_builtin(id)).collect();
        let mut team: Vec<TemplateSummary> = team_ids
            .iter()
            .filter_map(|id| self.resolve_for(id, viewer).ok())
            .map(|r| self.summary(&r))
            .collect();
        team.sort_by_key(|t| t.name.to_lowercase());
        out.extend(team);
        if let (true, Some(member)) = (viewer.personal, viewer.member_id()) {
            let mut own: Vec<TemplateSummary> =
                dirs_with_template(&profile_templates(&viewer.profiles, member))
                    .into_iter()
                    .filter(|id| !self.is_builtin(id) && !team_ids.contains(id))
                    .filter_map(|id| self.resolve_for(&id, viewer).ok())
                    .map(|r| self.summary(&r))
                    .collect();
            own.sort_by_key(|t| t.name.to_lowercase());
            out.extend(own);
        }
        out
    }

    fn summary(&self, r: &Resolved) -> TemplateSummary {
        let t = &r.loaded.template;
        TemplateSummary {
            id: t.id.clone(),
            name: t.name.clone(),
            description: t.description.clone(),
            icon: t.icon.clone(),
            version: t.version.clone(),
            builtin: self.is_builtin(&t.id),
            document: t.document,
            headings: t.sections.iter().map(|s| 1 + s.parts.len()).sum(),
            standards: t.standards.len(),
            layer: r.layer,
            edited: r.layer != Layer::Builtin && r.original.is_some(),
            shared: !(r.layer == Layer::Personal && r.original.is_none()),
            original_changed: r.original_changed.clone(),
        }
    }

    // ---- saving versions ------------------------------------------------------------

    /// The folder a layer keeps an id in, for this person.
    fn layer_dir(&self, viewer: &Viewer, id: &str, layer: Layer) -> Result<Dir, EngineError> {
        match layer {
            Layer::Team => self.team_dir(id),
            Layer::Personal => {
                check_id(id)?;
                viewer
                    .personal_dir(id)
                    .ok_or_else(|| EngineError::Template("Personal versions aren't in use here.".into()))
            }
            Layer::Builtin => Err(EngineError::Template("Built-in templates ship with OpenBark.".into())),
        }
    }

    /// Is the id used anywhere on this computer: built in, in the team folder,
    /// or in anyone's profile?
    fn id_taken(&self, id: &str, profiles: &Path) -> bool {
        self.is_builtin(id)
            || self.root.join(id).join(TEMPLATE_FILE).is_file()
            || (!profiles.as_os_str().is_empty()
                && fs::read_dir(profiles)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|m| m.path().join("templates").join(id).join(TEMPLATE_FILE).is_file()))
    }

    /// A new id from a name that no built-in, team template or profile uses.
    pub fn unique_id(&self, name: &str, profiles: &Path) -> String {
        let base = crate::template::id_from(name);
        let base: String = base.chars().take(44).collect::<String>().trim_end_matches('-').to_string();
        let mut id = base.clone();
        let mut n = 2;
        while self.id_taken(&id, profiles) {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Save a change: a new template, the first version of an original, or
    /// the next version. Working alone, or for the hospital, `target` is
    /// `Team`; in a team each person's own changes go to `Personal`.
    pub fn save_version(&self, viewer: &Viewer, req: SaveRequest) -> Result<Resolved, EngineError> {
        let mut incoming = req.template;
        incoming.standards.clear();
        incoming.schema = None; // versions always use the generated schema
        incoming.origin = None;
        let id = incoming.id.trim().to_string();
        let exists = !id.is_empty()
            && check_id(&id).is_ok()
            && (self.is_builtin(&id)
                || self.root.join(&id).join(TEMPLATE_FILE).is_file()
                || viewer.personal_dir(&id).is_some_and(|d| d.has(TEMPLATE_FILE)));
        if !exists {
            // A new template: never take over an id in use anywhere on this computer.
            let wanted = if id.is_empty() { incoming.name.clone() } else { id };
            incoming.id = self.unique_id(&wanted, &viewer.profiles);
            let mut t = LoadedTemplate::new(incoming)?.template;
            let new_id = t.id.clone();
            let dir = self.layer_dir(viewer, &new_id, req.target)?;
            t.version = dir.first_version();
            t.origin = Some(Origin {
                owner: if req.target == Layer::Personal { viewer.member.clone() } else { None },
                saved_by: req.saved_by,
                approved_by: req.approved_by,
                ..stamp_origin(req.target, &t)
            });
            dir.write(TEMPLATE_FILE, &to_yaml(&t))?;
            return self.resolve_for(&new_id, viewer);
        }
        // An existing id: refuse a save started from something other than what's there now.
        let current = self.resolve_for(&id, viewer)?;
        if let Some(base) = &req.base_version {
            let layer_ok = req.base_layer.is_none_or(|l| l == current.layer);
            if !layer_ok || *base != current.loaded.template.version {
                return Err(EngineError::Template(CONFLICT.into()));
            }
        }
        let dir = self.layer_dir(viewer, &id, req.target)?;
        let at_target = dir.load(&id, TEMPLATE_FILE)?;
        let original = self.original_of(&id, req.target)?;
        if at_target.is_none() && original.is_none() {
            return Err(EngineError::Template(
                "Only you use this template. Share it with the hospital instead.".into(),
            ));
        }
        incoming.id = id.clone();
        let mut t = LoadedTemplate::new(incoming)?.template;
        // Locked in a version of an original (ADR-0024 section 3).
        let locked_by = original.as_ref().map(|(o, _)| o.clone()).or_else(|| {
            at_target.as_ref().filter(|a| a.origin.as_ref().is_some_and(|o| o.based_on.is_some())).cloned()
        });
        if let Some(o) = &locked_by {
            if t.document != o.document {
                return Err(EngineError::Template(DOCUMENT_LOCKED.into()));
            }
            if t.fixed_rules != self.fixed_rules_for(&id, &o.fixed_rules) {
                return Err(EngineError::Template(FIXED_RULES_LOCKED.into()));
            }
        }
        let saved_now = match (&at_target, &original) {
            (Some(a), _) => Template { fixed_rules: self.fixed_rules_for(&id, &a.fixed_rules), ..a.clone() },
            (None, Some((o, _))) => o.clone(),
            (None, None) => unreachable!("checked above"),
        };
        let changed = fingerprint(&t) != fingerprint(&saved_now);
        if changed {
            match (&at_target, &original) {
                (Some(a), _) => {
                    let old = a.origin.clone().unwrap_or_else(|| blank_origin(req.target, &a.version));
                    if let Some(text) = dir.read(TEMPLATE_FILE)? {
                        dir.write(&history_file(&entry_name(&a.version)), &text)?;
                    }
                    t.version = next_version(&a.version);
                    t.origin = Some(Origin {
                        owner: old.owner.clone().or_else(|| {
                            if req.target == Layer::Personal { viewer.member.clone() } else { None }
                        }),
                        based_on: old.based_on.clone(),
                        seen_original: old.seen_original.clone(),
                        saved_by: req.saved_by,
                        approved_by: req.approved_by,
                        ..stamp_origin(req.target, &t)
                    });
                }
                (None, Some((o, o_layer))) => {
                    dir.write(BASE_FILE, &to_yaml(o))?;
                    t.version = dir.first_version();
                    t.origin = Some(Origin {
                        owner: if req.target == Layer::Personal { viewer.member.clone() } else { None },
                        based_on: Some(BasedOn {
                            layer: *o_layer,
                            id: id.clone(),
                            version: o.version.clone(),
                            fingerprint: fingerprint(o),
                        }),
                        saved_by: req.saved_by,
                        approved_by: req.approved_by,
                        ..stamp_origin(req.target, &t)
                    });
                }
                (None, None) => unreachable!("checked above"),
            }
            dir.write(TEMPLATE_FILE, &to_yaml(&t))?;
            dir.prune()?;
        }
        // Saved for the hospital from one's own version: that version now
        // matches the original, so it moves to history and stops applying.
        let retired = req.target == Layer::Team && current.layer == Layer::Personal;
        if retired {
            self.reset_version(viewer, &id, Layer::Personal)?;
        }
        if changed || retired { self.resolve_for(&id, viewer) } else { Ok(current) }
    }

    // ---- reset, undo, keep and delete -------------------------------------------------

    /// Stop using a version: it moves into history (as new files) and the
    /// original applies again. Returns a token for Undo.
    pub fn reset_version(&self, viewer: &Viewer, id: &str, layer: Layer) -> Result<UndoToken, EngineError> {
        let dir = self.layer_dir(viewer, id, layer)?;
        let Some(text) = dir.read(TEMPLATE_FILE)? else {
            return Err(EngineError::Template(NOTHING_TO_RESET.into()));
        };
        if self.original_of(id, layer)?.is_none() {
            return Err(EngineError::Template(match layer {
                Layer::Personal => {
                    "Only you use this template, so there's no original to go back to. Delete it instead."
                        .into()
                }
                _ => "This template has no original to go back to. Delete it instead.".into(),
            }));
        }
        let version = dir.load(id, TEMPLATE_FILE)?.map(|t| t.version).unwrap_or_else(|| "1".into());
        let entry = entry_name(&version);
        dir.write(&history_file(&entry), &text)?;
        match dir.read(BASE_FILE)? {
            Some(base) => dir.write(&history_base(&entry), &base)?,
            None => dir.remove(&history_base(&entry))?,
        }
        dir.remove(TEMPLATE_FILE)?;
        dir.remove(BASE_FILE)?;
        dir.prune()?;
        Ok(UndoToken { layer, id: id.to_string(), entry })
    }

    /// Bring back the version a reset moved into history.
    pub fn undo_reset(&self, viewer: &Viewer, token: &UndoToken) -> Result<Resolved, EngineError> {
        let id = token.id.as_str();
        let dir = self.layer_dir(viewer, id, token.layer)?;
        if dir.has(TEMPLATE_FILE) {
            return Err(EngineError::Template(
                "This template changed after it was reset, so the reset can't be undone.".into(),
            ));
        }
        let entry = entry_name(&token.entry);
        let Some(text) = dir.read(&history_file(&entry))? else {
            return Err(EngineError::Template("That reset was already undone.".into()));
        };
        if let Some(base) = dir.read(&history_base(&entry))? {
            dir.write(BASE_FILE, &base)?;
        }
        dir.write(TEMPLATE_FILE, &text)?;
        dir.remove(&history_file(&entry))?;
        dir.remove(&history_base(&entry))?;
        self.resolve_for(id, viewer)
    }

    /// "Keep mine": the original's current fingerprint counts as seen until
    /// it changes again. The version number doesn't change.
    pub fn keep_version(
        &self,
        viewer: &Viewer,
        id: &str,
        layer: Layer,
        seen: &str,
    ) -> Result<Resolved, EngineError> {
        let dir = self.layer_dir(viewer, id, layer)?;
        let Some(mut t) = dir.load(id, TEMPLATE_FILE)? else {
            return Err(EngineError::Template(NOTHING_TO_RESET.into()));
        };
        let mut origin = t.origin.clone().unwrap_or_else(|| blank_origin(layer, &t.version));
        origin.seen_original = Some(seen.to_string());
        t.origin = Some(origin);
        dir.write(TEMPLATE_FILE, &to_yaml(&t))?;
        self.resolve_for(id, viewer)
    }

    /// Delete a template only this person uses (`Personal`) or a team
    /// template (`Team`). Versions of an original are reset instead.
    pub fn delete_template(&self, viewer: &Viewer, id: &str, layer: Layer) -> Result<(), EngineError> {
        let dir = self.layer_dir(viewer, id, layer)?;
        if !dir.has(TEMPLATE_FILE) {
            return Err(EngineError::Template(if self.is_builtin(id) {
                format!("'{id}' is a built-in template and can't be deleted.")
            } else {
                format!("There's no template called {id} to delete.")
            }));
        }
        if self.original_of(id, layer)?.is_some() {
            return Err(EngineError::Template(match layer {
                Layer::Personal => {
                    "This is your version of a template. Reset it to the original instead.".into()
                }
                _ => "This is an edited built-in template. Reset it to the built-in instead.".into(),
            }));
        }
        fs::remove_dir_all(&dir.path).map_err(io(&dir.path))
    }

    /// Delete a team template and its standards.
    pub fn delete(&self, id: &str) -> Result<(), EngineError> {
        self.delete_template(&Viewer::alone(), id, Layer::Team)
    }

    /// Save a team template (working alone). Its version goes up when it changes.
    pub fn save(&self, template: Template) -> Result<LoadedTemplate, EngineError> {
        let req = SaveRequest {
            template,
            base_version: None,
            base_layer: None,
            target: Layer::Team,
            saved_by: None,
            approved_by: None,
        };
        Ok(self.save_version(&Viewer::alone(), req)?.loaded)
    }

    // ---- sharing ----------------------------------------------------------------------

    /// Share this person's version with the team (ADR-0024 section 6).
    /// `Replace` makes it the team's version of that id (a template only they
    /// used keeps its id) and retires the personal one; `Add` makes a new team
    /// template and keeps theirs.
    pub fn share(
        &self,
        viewer: &Viewer,
        id: &str,
        mode: ShareMode,
        name: Option<&str>,
        saved_by: Option<Who>,
        approved_by: Option<Who>,
    ) -> Result<Resolved, EngineError> {
        let mine_dir = self.layer_dir(viewer, id, Layer::Personal)?;
        let Some(mine) = mine_dir.load(id, TEMPLATE_FILE)? else {
            return Err(EngineError::Template(
                "You don't have your own version of this template to share.".into(),
            ));
        };
        let mut t = mine.clone();
        t.origin = None;
        t.fixed_rules = self.fixed_rules_for(id, &mine.fixed_rules);
        let req = |template: Template| SaveRequest {
            template,
            base_version: None,
            base_layer: None,
            target: Layer::Team,
            saved_by: saved_by.clone(),
            approved_by: approved_by.clone(),
        };
        match mode {
            ShareMode::Replace if self.original_of(id, Layer::Personal)?.is_some() => {
                // The team's next version (or first version of a built-in); then
                // the personal version, now the same as the original, retires.
                self.save_version(&viewer.shared_view(), req(t))?;
                self.reset_version(viewer, id, Layer::Personal)?;
                self.resolve_for(id, viewer)
            }
            ShareMode::Replace => {
                // A template only this person used: the team's now, with the same id.
                let team = self.team_dir(id)?;
                t.version = team.first_version();
                t.origin = Some(Origin { saved_by, approved_by, ..stamp_origin(Layer::Team, &t) });
                team.write(TEMPLATE_FILE, &to_yaml(&t))?;
                mine_dir.remove(TEMPLATE_FILE)?;
                mine_dir.remove(BASE_FILE)?;
                self.resolve_for(id, viewer)
            }
            ShareMode::Add => {
                t.name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&mine.name).to_string();
                t.id = String::new();
                let added = self.save_version(&viewer.shared_view(), req(t))?;
                self.resolve_for(&added.loaded.template.id, viewer)
            }
        }
    }

    /// Reset the team's version of a built-in: the team drafts with the
    /// built-in again. Returns a token for Undo.
    pub fn reset_team(&self, id: &str) -> Result<UndoToken, EngineError> {
        self.reset_version(&Viewer::alone(), id, Layer::Team)
    }

    // ---- comparing --------------------------------------------------------------------

    /// "What changed in the original" (`with_base`: the original when this
    /// version was made, against the original now) or "Yours and the
    /// original" (the original now, against this version).
    pub fn compare(&self, viewer: &Viewer, id: &str, with_base: bool) -> Result<TemplateDiff, EngineError> {
        let current = self.resolve_for(id, viewer)?;
        let Some((mut original, _)) = self.original_of(id, current.layer)? else {
            return Err(EngineError::Template("There's no original to compare with.".into()));
        };
        original.fixed_rules = self.fixed_rules_for(id, &original.fixed_rules);
        original.origin = None;
        if with_base {
            let dir = self.layer_dir(viewer, id, current.layer)?;
            let Some(mut base) = dir.load(id, BASE_FILE)? else {
                return Err(EngineError::Template(
                    "There's no copy of the original from when this version was made.".into(),
                ));
            };
            base.origin = None;
            Ok(template_diff(&base, &original))
        } else {
            let mut mine = current.loaded.template.clone();
            mine.standards.clear();
            mine.origin = None;
            Ok(template_diff(&original, &mine))
        }
    }

    // ---- profiles ---------------------------------------------------------------------

    /// What a team member keeps in their profile.
    pub fn profile_summary(&self, profiles: &Path, member: &str) -> ProfileSummary {
        let mut out = ProfileSummary::default();
        if !valid_member(member) {
            return out;
        }
        for entry in fs::read_dir(profile_templates(profiles, member)).into_iter().flatten().flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            let dir = Dir::sealed(entry.path());
            if dir.has(TEMPLATE_FILE) {
                if self.is_builtin(&id) || self.root.join(&id).join(TEMPLATE_FILE).is_file() {
                    out.versions += 1;
                } else {
                    out.own_templates += 1;
                }
            }
            if let Ok(Some(text)) = dir.read(STANDARDS_FILE)
                && let Ok(p) = serde_yaml_ng::from_str::<PersonalStandards>(&text)
            {
                out.standards += p.standards.len();
            }
        }
        out
    }

    /// Remove a team member's profile: their versions and standards.
    pub fn remove_profile(&self, profiles: &Path, member: &str) -> Result<(), EngineError> {
        if !valid_member(member) {
            return Err(EngineError::Template("That isn't a team member's id.".into()));
        }
        let dir = profiles.join(member);
        match fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(&dir)(e)),
            _ => Ok(()),
        }
    }

    /// Whose private template an id is (a template only they use), and its name.
    pub fn private_owner(&self, profiles: &Path, id: &str) -> Option<(String, String)> {
        if check_id(id).is_err() || self.is_builtin(id) || self.root.join(id).join(TEMPLATE_FILE).is_file() {
            return None;
        }
        fs::read_dir(profiles).into_iter().flatten().flatten().find_map(|m| {
            let member = m.file_name().to_string_lossy().into_owned();
            let dir = Dir::sealed(m.path().join("templates").join(id));
            let t = dir.load(id, TEMPLATE_FILE).ok().flatten()?;
            Some((member, t.name))
        })
    }

    // ---- standards --------------------------------------------------------------------

    /// The team's standards for an id.
    pub fn standards(&self, id: &str) -> Result<Vec<Standard>, EngineError> {
        let path = self.dir(id)?.join(STANDARDS_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => serde_yaml_ng::from_str(&text)
                .map_err(|e| EngineError::Template(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(io(&path)(e)),
        }
    }

    fn personal_standards(&self, viewer: &Viewer, id: &str) -> Result<PersonalStandards, EngineError> {
        let Some(dir) = viewer.personal_dir(id) else { return Ok(PersonalStandards::default()) };
        match dir.read(STANDARDS_FILE)? {
            Some(text) => serde_yaml_ng::from_str(&text)
                .map_err(|e| EngineError::Template(format!("{}: {e}", dir.file(STANDARDS_FILE).display()))),
            None => Ok(PersonalStandards::default()),
        }
    }

    fn write_personal_standards(
        &self,
        viewer: &Viewer,
        id: &str,
        p: &PersonalStandards,
    ) -> Result<(), EngineError> {
        let dir = self.layer_dir(viewer, id, Layer::Personal)?;
        dir.write(STANDARDS_FILE, &serde_yaml_ng::to_string(p).expect("standards serialise"))
    }

    /// Save a re-prompt as a standard for a heading ("" = the whole note), for
    /// everyone (the team's standards file).
    pub fn add_standard(
        &self,
        id: &str,
        heading: &str,
        text: &str,
        by: Option<&str>,
    ) -> Result<Standard, EngineError> {
        self.add_standard_for(&Viewer::alone(), id, heading, text, by, Scope::Team)
    }

    /// Save a standard for this person (`Personal`) or for everyone (`Team`).
    /// The heading must exist where the standard applies: in this person's
    /// template for themselves, in the team's or built-in one for everyone.
    pub fn add_standard_for(
        &self,
        viewer: &Viewer,
        id: &str,
        heading: &str,
        text: &str,
        by: Option<&str>,
        scope: Scope,
    ) -> Result<Standard, EngineError> {
        let applies_to = match scope {
            Scope::Personal => self.resolve_for(id, viewer)?,
            Scope::Team => self.resolve_for(id, &viewer.shared_view())?,
        };
        let template = applies_to.loaded;
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
        let standard = Standard {
            id: uuid::Uuid::now_v7().to_string(),
            heading: heading.to_string(),
            text: text.to_string(),
            added_at: Local::now(),
            added_by: by.map(str::trim).filter(|b| !b.is_empty()).map(String::from),
            scope,
        };
        let same = |s: &&Standard| s.heading == heading && s.text.eq_ignore_ascii_case(text);
        match scope {
            Scope::Team => {
                let mut all = self.standards(id)?;
                if let Some(existing) = all.iter().find(same) {
                    return Ok(existing.clone());
                }
                all.push(standard.clone());
                self.write_standards(id, &all)?;
            }
            Scope::Personal => {
                let mut p = self.personal_standards(viewer, id)?;
                if let Some(existing) = p.standards.iter().find(same) {
                    return Ok(Standard { scope: Scope::Personal, ..existing.clone() });
                }
                p.standards.push(standard.clone());
                self.write_personal_standards(viewer, id, &p)?;
            }
        }
        Ok(standard)
    }

    /// Which list a standard is in: this person's own, or the team's.
    pub fn standard_scope(
        &self,
        viewer: &Viewer,
        id: &str,
        standard_id: &str,
    ) -> Result<Option<Scope>, EngineError> {
        if self.personal_standards(viewer, id)?.standards.iter().any(|s| s.id == standard_id) {
            return Ok(Some(Scope::Personal));
        }
        Ok(self.standards(id)?.iter().any(|s| s.id == standard_id).then_some(Scope::Team))
    }

    pub fn remove_standard(&self, id: &str, standard_id: &str) -> Result<(), EngineError> {
        self.remove_standard_for(&Viewer::alone(), id, standard_id)
    }

    /// Remove one of this person's standards, or a team standard.
    pub fn remove_standard_for(
        &self,
        viewer: &Viewer,
        id: &str,
        standard_id: &str,
    ) -> Result<(), EngineError> {
        let mut p = self.personal_standards(viewer, id)?;
        if p.standards.iter().any(|s| s.id == standard_id) {
            p.standards.retain(|s| s.id != standard_id);
            return self.write_personal_standards(viewer, id, &p);
        }
        let mut all = self.standards(id)?;
        let before = all.len();
        all.retain(|s| s.id != standard_id);
        if all.len() == before {
            return Err(EngineError::Template("That standard was already removed.".into()));
        }
        self.write_standards(id, &all)
    }

    /// Turn a team standard off (or back on) for this person only.
    pub fn mute_standard(
        &self,
        viewer: &Viewer,
        id: &str,
        standard_id: &str,
        muted: bool,
    ) -> Result<(), EngineError> {
        if !self.standards(id)?.iter().any(|s| s.id == standard_id) {
            return Err(EngineError::Template("That isn't one of the team's standards.".into()));
        }
        let mut p = self.personal_standards(viewer, id)?;
        p.muted.retain(|m| m != standard_id);
        if muted {
            p.muted.push(standard_id.to_string());
        }
        self.write_personal_standards(viewer, id, &p)
    }

    fn write_standards(&self, id: &str, all: &[Standard]) -> Result<(), EngineError> {
        let dir = self.dir(id)?;
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        let all: Vec<Standard> = all.iter().map(|s| Standard { scope: Scope::Team, ..s.clone() }).collect();
        let text = serde_yaml_ng::to_string(&all).expect("standards serialise");
        write_atomic(&dir.join(STANDARDS_FILE), &text)
    }

    /// A template's team folder, refusing ids that could escape the root.
    fn dir(&self, id: &str) -> Result<PathBuf, EngineError> {
        check_id(id)?;
        Ok(self.root.join(id))
    }
}

/// Folder names under `root` that hold a template.
fn dirs_with_template(root: &Path) -> Vec<String> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join(TEMPLATE_FILE).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|id| check_id(id).is_ok())
        .collect()
}

/// Ids that could escape a folder are refused, for every layer.
fn check_id(id: &str) -> Result<(), EngineError> {
    let ok = !id.is_empty()
        && id.len() <= 48
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-');
    if ok { Ok(()) } else { Err(EngineError::Template(format!("'{id}' is not a template id."))) }
}

fn blank_origin(layer: Layer, version: &str) -> Origin {
    Origin {
        layer,
        owner: None,
        version: version.to_string(),
        fingerprint: String::new(),
        based_on: None,
        seen_original: None,
        saved_by: None,
        approved_by: None,
        saved_at: None,
        object_id: None,
    }
}

/// A fresh origin for a version being saved now.
fn stamp_origin(layer: Layer, t: &Template) -> Origin {
    Origin {
        fingerprint: fingerprint(t),
        saved_at: Some(Local::now()),
        object_id: Some(uuid::Uuid::now_v7().to_string()),
        ..blank_origin(layer, &t.version)
    }
}

/// Did the original change since the version was based on it (or kept)?
fn changed_original(
    based_on: &BasedOn,
    seen: Option<&str>,
    now: Option<&OriginalInfo>,
    based_name: impl Fn() -> String,
) -> Option<OriginalChanged> {
    match now {
        None => Some(OriginalChanged {
            reason: if based_on.layer == Layer::Builtin {
                OriginalChange::NoLongerBuiltin
            } else {
                OriginalChange::TeamDeleted
            },
            name: based_name(),
            version: String::new(),
            fingerprint: String::new(),
        }),
        Some(o) if o.fingerprint != based_on.fingerprint && Some(o.fingerprint.as_str()) != seen => {
            let reason = match (based_on.layer, o.layer) {
                (_, Layer::Team | Layer::Personal) => OriginalChange::TeamChanged,
                (Layer::Builtin, Layer::Builtin) => OriginalChange::NewerBuiltin,
                (_, Layer::Builtin) => OriginalChange::TeamReset,
            };
            Some(OriginalChanged {
                reason,
                name: o.name.clone(),
                version: o.version.clone(),
                fingerprint: o.fingerprint.clone(),
            })
        }
        _ => None,
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
mod tests;
