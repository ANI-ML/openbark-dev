//! Templates in the app (ADR-0024, with the owner's decisions in section 17).
//!
//! - Working alone (no team): every change goes straight into the templates
//!   folder (D1), with no PIN.
//! - A team, "each person's own" (the default): a veterinarian's or
//!   technician's changes are saved as their own version; the hospital's
//!   templates (the team layer) are everyone's defaults.
//! - A team, "shared by the hospital": everyone drafts with the hospital's
//!   templates. Personal versions are kept on disk, not used, and come back
//!   when the setting goes back.
//! - Only an admin (a flag on a team member, separate from the role) changes
//!   the hospital's templates or the setting, and every such change needs an
//!   admin's PIN, checked in the same call. The PIN travels in a request body
//!   only and is never logged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use os_engine::template::{
    CHECKS_AFTER_DRAFTING, CORE_RULE_SUMMARIES, CORE_RULES, Layer, LoadedTemplate, Origin, Scope, Standard,
    Template,
};
use os_engine::template_diff::TemplateDiff;
use os_engine::template_lint::{LintWarning, lint_template};
use os_engine::template_store::{
    OriginalChanged, OriginalInfo, ProfileSummary, Resolved, SaveRequest, ShareMode, TemplateSummary,
    UndoToken, Viewer,
};
use os_engine::users::{TeamSettings, TemplateMode, User, Who};
use serde::{Deserialize, Serialize};

use crate::{App, AppError, Need, Result};

/// The template the app drafts with when the chosen one isn't available.
pub const DEFAULT_TEMPLATE: &str = "soap";

const NEED_ADMIN: &str =
    "Changing the hospital's templates needs an admin. Ask one to approve with their PIN.";
const NOT_ADMIN: &str = "Only an admin can approve this.";
const HOSPITAL_ONLY: &str = "Templates are shared by the hospital, so only an admin can change them.";
const NO_TEAM: &str = "Set up a team first. Working alone, your templates are all yours.";

/// How templates work on this computer now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplatesMode {
    /// No team set up: the templates folder is yours.
    Alone,
    /// A team, each person's own versions.
    Personal,
    /// A team, everyone drafts with the hospital's templates.
    Hospital,
}

/// An admin's approval: their id and PIN, checked in the same engine call.
/// Request bodies only; written by hand so the PIN never reaches a log.
#[derive(Clone, Deserialize)]
pub struct Approver {
    pub id: String,
    pub pin: String,
}

impl std::fmt::Debug for Approver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approver").field("id", &self.id).field("pin", &"***").finish()
    }
}

/// What the person looking may do with a template (the editor's buttons).
#[derive(Debug, Clone, Default, Serialize)]
pub struct TemplateCan {
    /// Change it in the editor.
    pub edit: bool,
    /// Where Save puts the change: "personal" (their version) or "team" (the
    /// hospital's templates, or the templates folder working alone).
    pub save_to: Option<Layer>,
    /// Saving needs an admin's PIN (the hospital's templates in a team).
    pub save_needs_pin: bool,
    /// An admin can also save straight to the hospital's templates.
    pub save_for_hospital: bool,
    /// Reset their version (or, working alone, the folder's copy) to the original.
    pub reset: bool,
    /// Share their version with the hospital (an admin approves with a PIN).
    pub share: bool,
    /// Reset the hospital's version of a built-in (an admin, with their PIN).
    pub reset_team: bool,
    pub delete: bool,
    pub delete_needs_pin: bool,
    /// "Keep mine" when the original changed.
    pub keep: bool,
    pub keep_needs_pin: bool,
    pub duplicate: bool,
    /// Who it's for can change (a template with no original).
    pub change_document: bool,
    /// Save standards for themselves.
    pub standards_personal: bool,
    /// Saving a standard for everyone needs an admin's PIN.
    pub standards_team_needs_pin: bool,
    /// The person is an admin.
    pub admin: bool,
}

/// One rule that always applies, with its plain-language summary.
#[derive(Debug, Clone, Serialize)]
pub struct AppliedRule {
    /// The English sent to the model.
    pub text: String,
    /// In plain words (translated in the app).
    pub summary: String,
}

/// The "Always applied" panel: core rules, this template's fixed rules, and
/// the checks after drafting (ADR-0024 section 4.6).
#[derive(Debug, Clone, Serialize)]
pub struct AlwaysApplied {
    pub core: Vec<AppliedRule>,
    pub fixed: Vec<String>,
    pub checks: Vec<String>,
}

pub fn always_applied(fixed: &[String]) -> AlwaysApplied {
    AlwaysApplied {
        core: CORE_RULES
            .iter()
            .zip(CORE_RULE_SUMMARIES.iter())
            .map(|(text, summary)| AppliedRule { text: text.to_string(), summary: summary.to_string() })
            .collect(),
        fixed: fixed.to_vec(),
        checks: CHECKS_AFTER_DRAFTING.iter().map(|c| c.to_string()).collect(),
    }
}

/// A template for the editor: its headings, what the model is told, where it
/// comes from, and what this person may do with it.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateView {
    pub template: Template,
    /// The id is a built-in.
    pub builtin: bool,
    /// The system prompt drafts with this template start from.
    pub prompt: String,
    pub layer: Layer,
    pub origin: Option<Origin>,
    pub original: Option<OriginalInfo>,
    pub original_changed: Option<OriginalChanged>,
    /// A copy of the original from when this version was made is kept.
    pub has_base: bool,
    /// The hospital has its own version of this built-in.
    pub team_version: bool,
    pub can: TemplateCan,
    pub always_applied: AlwaysApplied,
    pub lint: Vec<LintWarning>,
    /// Standards for headings this template doesn't have (not applied).
    pub not_used: Vec<Standard>,
    /// Team standards this person turned off for themselves.
    pub muted: Vec<String>,
    pub mode: TemplatesMode,
    /// A preview of a draft that can't be drafted with yet (no name, a heading
    /// with no title): why, in plain words. What they may do still comes with it.
    pub problem: Option<String>,
}

/// A reset, and the token that undoes it (for the toast's Undo).
#[derive(Debug, Clone, Serialize)]
pub struct ResetView {
    pub view: TemplateView,
    pub undo: String,
}

/// The team's settings, as the app shows them.
#[derive(Debug, Clone, Serialize)]
pub struct TeamSettingsView {
    pub templates: TemplateMode,
    pub setup_notice: bool,
}

/// Save a template: `{template, by, base_version?, base_layer?, hospital?, approver?}`.
#[derive(Clone, Deserialize)]
pub struct SaveTemplateRequest {
    pub template: Template,
    #[serde(default)]
    pub by: Option<String>,
    /// The version the editor started from; a different one is refused.
    #[serde(default)]
    pub base_version: Option<String>,
    #[serde(default)]
    pub base_layer: Option<Layer>,
    /// Save to the hospital's templates (an admin, with their PIN).
    #[serde(default)]
    pub hospital: bool,
    #[serde(default)]
    pub approver: Option<Approver>,
}

impl std::fmt::Debug for SaveTemplateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveTemplateRequest")
            .field("id", &self.template.id)
            .field("by", &self.by)
            .field("base_version", &self.base_version)
            .field("hospital", &self.hospital)
            .field("approver", &self.approver)
            .finish()
    }
}

/// An undo token handed out after a reset, until used (or 30 minutes pass).
pub(crate) struct PendingUndo {
    token: UndoToken,
    member: Option<String>,
    at: Instant,
}

const UNDO_FOR: Duration = Duration::from_secs(30 * 60);

/// Who is looking, and how templates work for them.
pub(crate) struct Ctx {
    viewer: Viewer,
    user: Option<User>,
    who: Option<Who>,
    mode: TemplatesMode,
}

impl Ctx {
    fn admin(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.admin)
    }

    fn writes_notes(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.role.can_write_notes())
    }

    fn member_id(&self) -> Option<String> {
        self.user.as_ref().map(|u| u.id.clone())
    }

    /// Where a new template of theirs goes: their own in "each person's own", else the team's.
    fn new_layer(&self) -> Layer {
        match self.mode {
            TemplatesMode::Personal if self.writes_notes() => Layer::Personal,
            _ => Layer::Team,
        }
    }
}

impl App {
    /// `<folder of the team file>/profiles`, worked out each time, so a test
    /// (or a server) that moves the team file moves the profiles too.
    pub fn profiles_root(&self) -> PathBuf {
        self.users.path().parent().map(|p| p.join("profiles")).unwrap_or_else(|| PathBuf::from("profiles"))
    }

    pub fn templates_mode(&self) -> TemplatesMode {
        if !self.users.is_team() {
            return TemplatesMode::Alone;
        }
        match self.users.settings().templates {
            TemplateMode::Personal => TemplatesMode::Personal,
            TemplateMode::Hospital => TemplatesMode::Hospital,
        }
    }

    /// The person drafting, as the template store sees them.
    pub(crate) fn viewer_of(&self, who: Option<&Who>) -> Viewer {
        let profiles = self.profiles_root();
        match (self.templates_mode(), who) {
            (TemplatesMode::Alone, _) | (_, None) => Viewer { profiles, ..Viewer::alone() },
            (mode, Some(who)) => Viewer::member(who.clone(), profiles, mode == TemplatesMode::Personal),
        }
    }

    /// Anyone may look. With a team and no one chosen, the shared view.
    fn ctx(&self, by: Option<&str>) -> Result<Ctx> {
        let mode = self.templates_mode();
        let by = by.map(str::trim).filter(|b| !b.is_empty());
        let user = match mode {
            TemplatesMode::Alone => None,
            _ => by.and_then(|id| self.users.get(id)),
        };
        let who = match (&user, mode) {
            (Some(u), _) => Some(Who::from(u)),
            (None, TemplatesMode::Alone) => by.map(Who::named),
            (None, _) => None,
        };
        let viewer = match &user {
            Some(_) => self.viewer_of(who.as_ref()),
            None => Viewer { profiles: self.profiles_root(), ..Viewer::alone() },
        };
        Ok(Ctx { viewer, user, who, mode })
    }

    /// Changing anything: with a team, a team member must be chosen.
    pub(crate) fn writer(&self, by: Option<&str>) -> Result<Ctx> {
        let ctx = self.ctx(by)?;
        if ctx.mode != TemplatesMode::Alone && ctx.user.is_none() {
            return Err(AppError("Choose who you are first.".into()));
        }
        Ok(ctx)
    }

    /// Changing one's own version: each person's own templates, and a role that drafts notes.
    fn need_personal(&self, ctx: &Ctx) -> Result<()> {
        match ctx.mode {
            TemplatesMode::Hospital => Err(AppError(HOSPITAL_ONLY.into())),
            TemplatesMode::Alone => Ok(()),
            TemplatesMode::Personal if ctx.writes_notes() => Ok(()),
            TemplatesMode::Personal => Err(AppError(format!(
                "{} can read templates; changing them needs a veterinarian or technician.",
                ctx.user.as_ref().map(|u| u.name.as_str()).unwrap_or("This person")
            ))),
        }
    }

    /// A change to the hospital's templates or settings: working alone,
    /// nothing to approve; with a team, an admin's PIN in this same call.
    pub(crate) fn need_admin(&self, ctx: &Ctx, approver: Option<&Approver>) -> Result<Option<Who>> {
        if ctx.mode == TemplatesMode::Alone {
            return Ok(None);
        }
        let approver = approver.filter(|a| !a.pin.is_empty()).ok_or_else(|| AppError(NEED_ADMIN.into()))?;
        let admin = self.users.verify(&approver.id, &approver.pin)?;
        if !admin.admin {
            return Err(AppError(NOT_ADMIN.into()));
        }
        Ok(Some(Who::from(&admin)))
    }

    fn team_version(&self, id: &str) -> bool {
        self.templates.is_builtin(id) && self.templates.root().join(id).join("template.yaml").is_file()
    }

    fn can(&self, ctx: &Ctx, r: &Resolved) -> TemplateCan {
        self.can_for(ctx, &r.loaded.template.id, r.layer, r.original.is_some(), r.original_changed.is_some())
    }

    /// What this person may do with template `id` in `layer`. A new one has no
    /// id yet, so its permissions never depend on what's typed in it.
    fn can_for(&self, ctx: &Ctx, id: &str, layer: Layer, has_original: bool, changed: bool) -> TemplateCan {
        let own = layer == Layer::Personal && !has_original;
        let team_made = layer == Layer::Team && !has_original;
        let team_version = self.team_version(id);
        let base = TemplateCan { change_document: !has_original, admin: ctx.admin(), ..Default::default() };
        match ctx.mode {
            TemplatesMode::Alone => TemplateCan {
                edit: true,
                save_to: Some(Layer::Team),
                reset: layer == Layer::Team && has_original,
                delete: team_made,
                keep: layer == Layer::Team && changed,
                duplicate: true,
                ..base
            },
            TemplatesMode::Personal => {
                let writes = ctx.writes_notes();
                let admin = ctx.admin();
                TemplateCan {
                    edit: writes || admin,
                    save_to: if writes {
                        Some(Layer::Personal)
                    } else if admin {
                        Some(Layer::Team)
                    } else {
                        None
                    },
                    save_needs_pin: !writes && admin,
                    save_for_hospital: writes && admin && !own,
                    reset: writes && layer == Layer::Personal && has_original,
                    share: writes && layer == Layer::Personal,
                    reset_team: admin && team_version,
                    delete: (writes && own) || (admin && team_made),
                    delete_needs_pin: team_made,
                    keep: (writes && layer == Layer::Personal && changed)
                        || (admin && layer == Layer::Team && changed),
                    keep_needs_pin: layer == Layer::Team,
                    duplicate: writes || admin,
                    standards_personal: writes,
                    standards_team_needs_pin: true,
                    ..base
                }
            }
            TemplatesMode::Hospital => {
                let admin = ctx.admin();
                TemplateCan {
                    edit: admin,
                    save_to: admin.then_some(Layer::Team),
                    save_needs_pin: true,
                    reset_team: admin && team_version,
                    delete: admin && team_made,
                    delete_needs_pin: true,
                    keep: admin && layer == Layer::Team && changed,
                    keep_needs_pin: true,
                    duplicate: admin,
                    standards_team_needs_pin: true,
                    ..base
                }
            }
        }
    }

    /// The "newer original" notice goes to those who can act on it: not to a
    /// non-admin about the hospital's own version.
    fn shown_change(
        &self,
        ctx: &Ctx,
        layer: Layer,
        change: Option<OriginalChanged>,
    ) -> Option<OriginalChanged> {
        if layer == Layer::Team && ctx.mode != TemplatesMode::Alone && !ctx.admin() {
            return None;
        }
        change
    }

    fn view(&self, ctx: &Ctx, r: Resolved) -> TemplateView {
        let can = self.can(ctx, &r);
        let id = r.loaded.template.id.clone();
        let original_changed = self.shown_change(ctx, r.layer, r.original_changed.clone());
        TemplateView {
            prompt: os_engine::note::system_prompt(&r.loaded),
            lint: lint_template(&r.loaded.template),
            always_applied: always_applied(&r.loaded.template.fixed_rules),
            builtin: self.templates.is_builtin(&id),
            team_version: self.team_version(&id),
            layer: r.layer,
            origin: r.loaded.template.origin.clone(),
            original: r.original,
            original_changed,
            has_base: r.has_base,
            can,
            not_used: r.not_used,
            muted: r.muted,
            mode: ctx.mode,
            problem: None,
            template: r.loaded.template,
        }
    }

    /// A read-only view of a template that isn't in the store (a run's
    /// snapshot, or a proposal from an example note).
    fn loose_view(&self, loaded: LoadedTemplate, mode: TemplatesMode, editable: bool) -> TemplateView {
        let layer = loaded.template.origin.as_ref().map(|o| o.layer).unwrap_or(Layer::Builtin);
        TemplateView {
            prompt: os_engine::note::system_prompt(&loaded),
            lint: lint_template(&loaded.template),
            always_applied: always_applied(&loaded.template.fixed_rules),
            builtin: self.templates.is_builtin(&loaded.template.id),
            team_version: false,
            layer,
            origin: loaded.template.origin.clone(),
            original: None,
            original_changed: None,
            has_base: false,
            can: TemplateCan { edit: editable, change_document: editable, ..Default::default() },
            not_used: Vec::new(),
            muted: Vec::new(),
            mode,
            problem: None,
            template: loaded.template,
        }
    }

    fn remember_undo(&self, token: UndoToken, member: Option<String>) -> String {
        let key = os_engine::audit::new_id();
        if let Ok(mut map) = self.undo.lock() {
            map.retain(|_, p| p.at.elapsed() < UNDO_FOR);
            map.insert(key.clone(), PendingUndo { token, member, at: Instant::now() });
        }
        key
    }

    // ---- reading ------------------------------------------------------------------

    /// Every template this person can draft with.
    pub fn list_templates(&self, by: Option<&str>) -> Result<Vec<TemplateSummary>> {
        let ctx = self.ctx(by)?;
        Ok(self
            .templates
            .list_for(&ctx.viewer)
            .into_iter()
            .map(|mut s| {
                s.original_changed = self.shown_change(&ctx, s.layer, s.original_changed.take());
                s
            })
            .collect())
    }

    /// A template as this person gets it, with its prompt and what they may do.
    pub fn template(&self, id: &str, by: Option<&str>) -> Result<TemplateView> {
        let ctx = self.ctx(by)?;
        let r = self.templates.resolve_for(id, &ctx.viewer)?;
        Ok(self.view(&ctx, r))
    }

    /// Check an unsaved template and show what the model would be told: the
    /// fixed rules of the current built-in, and the standards that apply. A
    /// draft that can't be drafted with yet comes back with its `problem`, and
    /// with what this person may do: that comes from who they are and what's
    /// stored, never from the draft, so a new template is editable from the start.
    pub fn preview_template(&self, template: Template, by: Option<&str>) -> Result<TemplateView> {
        let ctx = self.ctx(by)?;
        let mut t = template;
        t.origin = None;
        if let Some(b) = self.templates.builtin(&t.id) {
            t.fixed_rules = b.template.fixed_rules;
        }
        let current =
            (!t.id.is_empty()).then(|| self.templates.resolve_for(&t.id, &ctx.viewer).ok()).flatten();
        let can = match &current {
            Some(r) => self.can(&ctx, r),
            None => self.can_for(&ctx, "", ctx.new_layer(), false, false),
        };
        let mut loaded = match LoadedTemplate::new(t.clone()) {
            Ok(loaded) => loaded,
            Err(e) => {
                return Ok(TemplateView {
                    prompt: String::new(),
                    lint: Vec::new(),
                    always_applied: always_applied(&t.fixed_rules),
                    builtin: self.templates.is_builtin(&t.id),
                    team_version: self.team_version(&t.id),
                    layer: current.as_ref().map_or_else(|| ctx.new_layer(), |r| r.layer),
                    origin: None,
                    original: None,
                    original_changed: None,
                    has_base: false,
                    can,
                    not_used: Vec::new(),
                    muted: Vec::new(),
                    mode: ctx.mode,
                    problem: Some(e.to_string()),
                    template: t,
                });
            }
        };
        match current {
            Some(r) => {
                let (applied, not_used): (Vec<Standard>, Vec<Standard>) = r
                    .loaded
                    .template
                    .standards
                    .iter()
                    .chain(r.not_used.iter())
                    .cloned()
                    .partition(|s| s.heading.is_empty() || loaded.heading_title(&s.heading).is_some());
                loaded.template.standards = applied;
                loaded.template.origin = r.loaded.template.origin.clone();
                Ok(TemplateView { can, ..self.view(&ctx, Resolved { loaded, not_used, ..r }) })
            }
            None => {
                let r = Resolved {
                    loaded,
                    layer: ctx.new_layer(),
                    original: None,
                    original_changed: None,
                    not_used: Vec::new(),
                    muted: Vec::new(),
                    has_base: false,
                };
                Ok(TemplateView { can, ..self.view(&ctx, r) })
            }
        }
    }

    /// A proposal from an example note (nothing is saved).
    pub(crate) fn proposal_view(&self, loaded: LoadedTemplate) -> TemplateView {
        self.loose_view(loaded, self.templates_mode(), true)
    }

    /// The template a visit's latest draft was written with, read-only
    /// ("Show the template" in Review).
    pub fn run_template_view(&self, session: &str, pet: Option<&str>) -> Result<TemplateView> {
        let s = self.open(session, pet)?;
        let run =
            s.latest_record().ok_or_else(|| AppError("There is no draft yet. Draft a note first.".into()))?;
        let loaded = s.run_template(run)?;
        Ok(self.loose_view(loaded, self.templates_mode(), false))
    }

    /// "What changed in the original" (`with = "base"`) or "Yours and the
    /// original" (`with = "original"`).
    pub fn compare_template(&self, id: &str, by: Option<&str>, with: &str) -> Result<TemplateDiff> {
        let ctx = self.ctx(by)?;
        Ok(self.templates.compare(&ctx.viewer, id, with == "base")?)
    }

    // ---- changing -----------------------------------------------------------------

    /// Save a change: this person's version, a new template only they see
    /// (D6), the hospital's template (an admin, with their PIN), or, working
    /// alone, the templates folder.
    pub fn save_template(&self, req: SaveTemplateRequest) -> Result<TemplateView> {
        let ctx = self.writer(req.by.as_deref())?;
        let (target, approved_by) = match ctx.mode {
            TemplatesMode::Alone => (Layer::Team, None),
            TemplatesMode::Personal if !req.hospital => {
                self.need_personal(&ctx)?;
                (Layer::Personal, None)
            }
            TemplatesMode::Hospital if !ctx.admin() => return Err(AppError(HOSPITAL_ONLY.into())),
            _ => (Layer::Team, self.need_admin(&ctx, req.approver.as_ref())?),
        };
        let r = self.templates.save_version(
            &ctx.viewer,
            SaveRequest {
                template: req.template,
                base_version: req.base_version,
                base_layer: req.base_layer,
                target,
                saved_by: ctx.who.clone(),
                approved_by,
            },
        )?;
        Ok(self.view(&ctx, r))
    }

    /// Reset this person's version (or, working alone, the folder's copy) to
    /// the original. The toast's Undo uses the token.
    pub fn reset_template(&self, id: &str, by: Option<&str>) -> Result<ResetView> {
        let ctx = self.writer(by)?;
        let layer = match ctx.mode {
            TemplatesMode::Alone => Layer::Team,
            TemplatesMode::Personal => {
                self.need_personal(&ctx)?;
                Layer::Personal
            }
            TemplatesMode::Hospital => return Err(AppError(HOSPITAL_ONLY.into())),
        };
        let token = self.templates.reset_version(&ctx.viewer, id, layer)?;
        let undo = self.remember_undo(token, ctx.member_id());
        let r = self.templates.resolve_for(id, &ctx.viewer)?;
        Ok(ResetView { view: self.view(&ctx, r), undo })
    }

    /// Reset the hospital's version of a built-in (an admin, with their PIN).
    pub fn reset_team_template(
        &self,
        id: &str,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<ResetView> {
        let ctx = self.writer(by)?;
        self.need_admin(&ctx, approver)?;
        let token = self.templates.reset_team(id)?;
        let undo = self.remember_undo(token, ctx.member_id());
        let r = self.templates.resolve_for(id, &ctx.viewer)?;
        Ok(ResetView { view: self.view(&ctx, r), undo })
    }

    /// Undo a reset this person just made. Each token works once.
    pub fn undo_reset_template(&self, id: &str, by: Option<&str>, undo: &str) -> Result<TemplateView> {
        let ctx = self.writer(by)?;
        let gone = || AppError("That reset can't be undone any more.".into());
        let pending = {
            let mut map = self.undo.lock().map_err(|_| AppError("undo lock poisoned".into()))?;
            let ok = map.get(undo).is_some_and(|p| {
                p.at.elapsed() < UNDO_FOR && p.token.id == id && p.member == ctx.member_id()
            });
            if !ok {
                return Err(gone());
            }
            map.remove(undo).ok_or_else(gone)?
        };
        let r = self.templates.undo_reset(&ctx.viewer, &pending.token)?;
        Ok(self.view(&ctx, r))
    }

    /// "Keep mine": the original's change counts as seen.
    pub fn keep_template(
        &self,
        id: &str,
        by: Option<&str>,
        fingerprint: &str,
        approver: Option<&Approver>,
    ) -> Result<TemplateView> {
        let ctx = self.writer(by)?;
        let layer = self.templates.resolve_for(id, &ctx.viewer)?.layer;
        match layer {
            Layer::Personal => self.need_personal(&ctx)?,
            Layer::Team => {
                self.need_admin(&ctx, approver)?;
            }
            Layer::Builtin => return Err(AppError("You're already using the original.".into())),
        }
        let r = self.templates.keep_version(&ctx.viewer, id, layer, fingerprint)?;
        Ok(self.view(&ctx, r))
    }

    /// Share this person's version with the hospital; an admin approves with their PIN.
    pub fn share_template(
        &self,
        id: &str,
        by: Option<&str>,
        mode: ShareMode,
        name: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<TemplateView> {
        let ctx = self.writer(by)?;
        if ctx.mode == TemplatesMode::Alone {
            return Err(AppError(NO_TEAM.into()));
        }
        self.need_personal(&ctx)?;
        let approved_by = self.need_admin(&ctx, approver)?;
        let r = self.templates.share(&ctx.viewer, id, mode, name, ctx.who.clone(), approved_by)?;
        Ok(self.view(&ctx, r))
    }

    /// Delete a template only this person uses, or (an admin, with their PIN)
    /// one the hospital made.
    pub fn delete_template(&self, id: &str, by: Option<&str>, approver: Option<&Approver>) -> Result<()> {
        let ctx = self.writer(by)?;
        let r = self.templates.resolve_for(id, &ctx.viewer)?;
        match r.layer {
            Layer::Builtin => {
                return Err(AppError(format!(
                    "{} is built in and can't be deleted.",
                    r.loaded.template.name
                )));
            }
            Layer::Personal => self.need_personal(&ctx)?,
            Layer::Team => {
                self.need_admin(&ctx, approver)?;
            }
        }
        Ok(self.templates.delete_template(&ctx.viewer, id, r.layer)?)
    }

    // ---- standards ----------------------------------------------------------------

    /// The scope a standard is saved with, and who approved it, checked
    /// before anything is drafted or written.
    fn standard_permission(&self, ctx: &Ctx, scope: Scope, approver: Option<&Approver>) -> Result<Scope> {
        match (ctx.mode, scope) {
            // Working alone, every standard is everyone's.
            (TemplatesMode::Alone, _) => Ok(Scope::Team),
            (TemplatesMode::Hospital, Scope::Personal) => Err(AppError(
                "Standards for yourself are off while templates are shared by the hospital.".into(),
            )),
            (TemplatesMode::Personal, Scope::Personal) => {
                self.need_personal(ctx)?;
                Ok(Scope::Personal)
            }
            (_, Scope::Team) => {
                self.need_admin(ctx, approver)?;
                Ok(Scope::Team)
            }
        }
    }

    /// The scope a standard gets when none is chosen.
    fn default_scope(&self) -> Scope {
        if self.templates_mode() == TemplatesMode::Personal { Scope::Personal } else { Scope::Team }
    }

    /// Save a request as a standard for a heading ("" = the whole note), for
    /// this person or (with an admin's PIN) for everyone.
    pub fn add_standard(
        &self,
        id: &str,
        heading: &str,
        text: &str,
        by: Option<&str>,
        scope: Option<Scope>,
        approver: Option<&Approver>,
    ) -> Result<Standard> {
        let ctx = self.writer(by)?;
        let scope =
            self.standard_permission(&ctx, scope.unwrap_or_else(|| self.default_scope()), approver)?;
        let name = ctx.who.as_ref().map(|w| w.name.clone());
        Ok(self.templates.add_standard_for(&ctx.viewer, id, heading, text, name.as_deref(), scope)?)
    }

    pub fn remove_standard(
        &self,
        id: &str,
        standard_id: &str,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<TemplateView> {
        let ctx = self.writer(by)?;
        match self.templates.standard_scope(&ctx.viewer, id, standard_id)? {
            None => return Err(AppError("That standard was already removed.".into())),
            Some(Scope::Personal) => self.need_personal(&ctx)?,
            Some(Scope::Team) => {
                self.need_admin(&ctx, approver)?;
            }
        }
        self.templates.remove_standard_for(&ctx.viewer, id, standard_id)?;
        self.template(id, by)
    }

    /// Turn one of the team's standards off (or on) for this person only.
    pub fn mute_standard(
        &self,
        id: &str,
        standard_id: &str,
        by: Option<&str>,
        muted: bool,
    ) -> Result<TemplateView> {
        let ctx = self.writer(by)?;
        if ctx.mode != TemplatesMode::Personal {
            return Err(AppError(
                "Standards can be turned off for yourself when templates are each person's own.".into(),
            ));
        }
        self.need_personal(&ctx)?;
        self.templates.mute_standard(&ctx.viewer, id, standard_id, muted)?;
        self.template(id, by)
    }

    /// Check a "save as a standard" request before redrafting (Review, Ask):
    /// the permission, the PIN, and that the heading exists where it applies.
    pub(crate) fn check_standard_request(
        &self,
        id: &str,
        heading: &str,
        by: Option<&str>,
        scope: Option<Scope>,
        approver: Option<&Approver>,
    ) -> Result<Scope> {
        let ctx = self.writer(by)?;
        let scope =
            self.standard_permission(&ctx, scope.unwrap_or_else(|| self.default_scope()), approver)?;
        let viewer = if scope == Scope::Team {
            Viewer { personal: false, ..ctx.viewer.clone() }
        } else {
            ctx.viewer.clone()
        };
        let t = self.templates.resolve_for(id, &viewer).map_err(|_| {
            AppError(
                "This template is no longer in your templates, so the request can't be saved as a standard."
                    .into(),
            )
        })?;
        if !heading.is_empty() && t.loaded.heading_title(heading).is_none() {
            return Err(AppError(format!(
                "{} has no heading {heading}, so it can't be saved as a standard.",
                t.loaded.template.name
            )));
        }
        Ok(scope)
    }

    pub(crate) fn save_standard_after_refine(
        &self,
        id: &str,
        heading: &str,
        text: &str,
        by: Option<&str>,
        scope: Scope,
    ) -> Result<Standard> {
        let ctx = self.writer(by)?;
        let name = ctx.who.as_ref().map(|w| w.name.clone());
        Ok(self.templates.add_standard_for(&ctx.viewer, id, heading, text, name.as_deref(), scope)?)
    }

    // ---- drafting -----------------------------------------------------------------

    /// The template someone drafts with: their resolution of the id. When the
    /// id is another person's private template, the default template is used
    /// instead, with a notice saying so.
    pub(crate) fn drafting_template(
        &self,
        id: &str,
        who: Option<&Who>,
    ) -> Result<(LoadedTemplate, Option<String>)> {
        let viewer = self.viewer_of(who);
        match self.templates.resolve_for(id, &viewer) {
            Ok(r) => Ok((r.loaded, None)),
            Err(e) => {
                let Some((owner, name)) = self.templates.private_owner(&viewer.profiles, id) else {
                    return Err(e.into());
                };
                let person = self.users.get(&owner).map(|u| u.name).unwrap_or_else(|| "someone else".into());
                let fallback = self.templates.resolve_for(DEFAULT_TEMPLATE, &viewer)?;
                let notice = format!(
                    "{name} is only available to {person}. Drafting with {}.",
                    fallback.loaded.template.name
                );
                Ok((fallback.loaded, Some(notice)))
            }
        }
    }

    // ---- the team's settings and admins ----------------------------------------------

    pub fn team_settings(&self) -> TeamSettingsView {
        let s = self.users.settings();
        TeamSettingsView { templates: s.templates, setup_notice: s.setup_notice && self.users.is_team() }
    }

    /// Switch between each person's own templates and the hospital's (an
    /// admin, with their PIN). Personal versions stay on disk either way.
    pub fn set_template_mode(
        &self,
        mode: TemplateMode,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<TeamSettingsView> {
        let ctx = self.writer(by)?;
        if ctx.mode == TemplatesMode::Alone {
            return Err(AppError(NO_TEAM.into()));
        }
        self.need_admin(&ctx, approver)?;
        let mut s = self.users.settings();
        s.templates = mode;
        self.users.set_settings(&s)?;
        Ok(self.team_settings())
    }

    /// The one-time notice after a team was set up, seen.
    pub fn dismiss_setup_notice(&self, by: Option<&str>) -> Result<TeamSettingsView> {
        let ctx = self.writer(by)?;
        if !ctx.admin() {
            return Err(AppError(NOT_ADMIN.into()));
        }
        let s = TeamSettings { setup_notice: false, ..self.users.settings() };
        self.users.set_settings(&s)?;
        Ok(self.team_settings())
    }

    /// Grant or remove the admin flag (an admin, with their PIN). There is
    /// always at least one admin.
    pub fn set_user_admin(
        &self,
        uid: &str,
        admin: bool,
        by: Option<&str>,
        approver: Option<&Approver>,
    ) -> Result<User> {
        let ctx = self.writer(by)?;
        if ctx.mode == TemplatesMode::Alone {
            return Err(AppError(NO_TEAM.into()));
        }
        self.need_admin(&ctx, approver)?;
        Ok(self.users.set_admin(uid, admin)?)
    }

    /// What a team member keeps in their profile, for the removal confirmation.
    pub fn profile_summary(&self, uid: &str, by: Option<&str>) -> Result<ProfileSummary> {
        self.who(by, Need::Team)?;
        Ok(self.templates.profile_summary(&self.profiles_root(), uid))
    }

    /// When the first member is added: tell the first admin, once, if the
    /// folder holds templates changed while working alone.
    pub(crate) fn note_team_setup(&self, was_team: bool) {
        if was_team || !self.users.is_team() {
            return;
        }
        let changed = self.templates.list().iter().any(|t| t.layer == Layer::Team);
        if changed {
            let s = TeamSettings { setup_notice: true, ..self.users.settings() };
            let _ = self.users.set_settings(&s);
        }
    }
}

/// Old undo tokens, keyed by the token handed out.
pub(crate) type UndoMap = HashMap<String, PendingUndo>;
