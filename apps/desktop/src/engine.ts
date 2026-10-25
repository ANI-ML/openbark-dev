// One engine interface over two transports (identical behaviour, os-app):
// - desktop (Tauri): commands in src-tauri/src/lib.rs, events via Tauri
// - web (`openbark serve`): HTTP API in engine/os-server, events via SSE,
//   and the browser's microphone streamed over a WebSocket
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { recordInBrowser } from "./mic/browser";
import type { MicHealth, MicQuality } from "./mic/health";

export interface Hint {
  heard: string;
  likely: string[];
  kind: string;
}

export interface Segment {
  id: number;
  start: number;
  end: number;
  speaker: number | null;
  text: string;
  /** Likely mis-heard veterinary terms (shown, never applied). */
  hints?: Hint[];
  /** What the second recogniser heard (segments with numbers or clinical terms). */
  check?: { text: string; primary_only: string[]; second_only: string[] };
}

export interface Transcript {
  segments: Segment[];
}

export interface TranscribeStats {
  audio_secs: number;
  total_ms: number;
  segments: number;
  speakers: number;
}

export interface NumberFlag {
  path: string;
  number: string;
  cited: number[];
  found_in: number[];
}

export interface Faithfulness {
  passed: boolean;
  statements_checked: number;
  numbers_checked: number;
  unsupported_numbers: NumberFlag[];
  citation_mismatches: NumberFlag[];
  disputed_numbers?: { path: string; number: string; heard_instead: string[]; cited: number[] }[];
  unverified_values?: string[];
  /** Statements whose words aren't in the lines they cite (word check, M2). */
  unsupported_statements?: { path: string; missing: string[]; cited: number[]; suggested: number[] }[];
  /** Items a veterinarian wrote or edited by hand (listed, not checked). */
  vet_entered?: string[];
  invalid_evidence: string[];
  /**
   * Flags on charted teeth (ADR-0021): soft (look before signing), except
   * not_in_mouth and choose_dentition, which block signing.
   */
  tooth_flags?: {
    path: string;
    tooth: string;
    kind:
      | "name_conflict"
      | "cue_missing"
      | "other_tooth"
      | "negated"
      | "other_type"
      | "planned"
      | "not_planned"
      | "history"
      | "uncertain"
      | "ambiguous"
      | "contradicted"
      | "other_pet"
      | "conflict"
      | "shape"
      | "duplicate"
      | "not_in_mouth"
      | "choose_dentition";
    other?: string;
    expected?: string[];
  }[];
}

export interface NoteRun {
  status: "draft";
  template: string;
  model: string;
  note: Record<string, unknown>;
  faithfulness: Faithfulness;
  stats: {
    prompt_tokens: number;
    completion_tokens: number;
    total_ms: number;
    tokens_per_second: number;
    time_to_first_token_ms: number;
  };
  /** Set when this draft re-prompted one heading of an earlier draft. */
  refinement?: Refinement;
  /** Set when this draft is a veterinarian's hand edit of one heading. */
  edit?: { heading: string; by: string; from_run?: string | null; changed: string[]; removed: number; before: unknown };
}

/** A dated, signed addition to a signed note. */
export interface Addendum {
  id: string;
  text: string;
  by: string;
  at: string;
}

/** Clinic-entered details, time-stamped by the engine. */
export interface Patient {
  name: string;
  last_name: string;
  reason: string;
  /** Other animals seen in the same visit (one note each). */
  others?: string[];
  /** The main animal's species key ("feline"); decides species-only headings. */
  species?: string;
  /** The main animal's patient record (ADR-0011), when chosen or linked. */
  patient_id?: string | null;
  /** The other animals' patient records, in the order of `others`. */
  other_ids?: string[];
  entered_at?: string | null;
  updated_at?: string | null;
}

export type ConsentRule = "all_parties" | "one_party";
export type ConsentMethod = "verbal" | "written" | "on_file" | "clinician_only" | "announced";
/** A phone call: what the client said after the announcement (ADR-0023 section 7). */
export type ConsentOutcome = "agreed" | "declined" | "stayed_on_line" | "not_announced";

/** Consent to record one encounter (M1 item 7); the engine stamps the time. */
export interface RecordingConsent {
  rule: ConsentRule;
  method: ConsentMethod;
  confirmed_by: string;
  confirmed_at?: string | null;
  script?: string | null;
  /** Phone calls: the client's answer after the announcement. */
  outcome?: ConsentOutcome | null;
  /** Phone calls: when the announcement started, in seconds into the recording. */
  announced_at_secs?: number | null;
}

/** A phone call's details (ADR-0023 section 8). Speaker 0 is the clinic, 1 the client. */
export interface CallInfo {
  direction: "outgoing" | "incoming";
  number: string;
  started_at: string;
  duration_secs: number;
  call_sid?: string | null;
  /** "Vet", "Tech" or "Clinic". */
  clinic_label: string;
  /** When the client declined and the recording was deleted, in seconds into the call. */
  declined_at_secs?: number | null;
}

const consentQuery = (c: RecordingConsent | null) => ({
  consent_rule: c?.rule ?? "",
  consent_method: c?.method ?? "",
  consent_by: c?.confirmed_by ?? "",
  consent_script: c?.script ?? "",
});

export const emptyPatient = (): Patient => ({ name: "", last_name: "", reason: "" });
const hasPatient = (p: Patient | null | undefined): p is Patient =>
  Boolean(p && (p.name.trim() || p.last_name.trim() || p.reason.trim()));

export interface SessionSummary {
  id: string;
  created_at: string;
  source: string;
  audio_secs: number;
  runs: number;
  signed: boolean;
  patient: string;
  reason: string;
  /** Name of the template the latest draft used. */
  template: string;
  /** Encrypted sync (ADR-0017): synced, pending or stopped; absent while sync is off. */
  sync?: SyncVisitState | null;
  /** A visit from another device: the device that holds its audio. */
  audio_on?: string | null;
  /** The animal's name and the family name, apart (ADR-0027); absent from older engines. */
  animal?: string;
  family?: string;
  /** Who signed the latest signed note, by name. */
  signed_by?: string | null;
}

export type SyncVisitState = "synced" | "pending" | "stopped";

export interface SessionView {
  id: string;
  dir: string;
  audio_path: string;
  created_at: string;
  source: string;
  transcript: Transcript | null;
  stats: TranscribeStats | null;
  latest_run: NoteRun | null;
  latest_markdown: string | null;
  signed_at: string | null;
  signed_by: string | null;
  patient: Patient | null;
  /** The template the latest draft was written with (saved with the draft). */
  latest_template: Template | null;
  /** Signed additions to the signed note, oldest first. */
  addenda: Addendum[];
  /** Consent to record, as confirmed before recording. */
  consent: RecordingConsent | null;
  /** The latest draft of each client document. */
  documents: DocumentView[];
  /** Who started the recording or import. */
  recorded_by: Who | null;
  /** The animals in a multi-pet visit (empty for a single animal). */
  pets: string[];
  /** Whose notes this view shows, in a multi-pet visit. */
  pet: string | null;
  /** Any note in the visit is signed, for any animal (ADR-0022). */
  any_signed?: boolean;
  /** The recording is still kept. */
  has_audio?: boolean;
  /** When only the recording was deleted, and by whom. */
  audio_deleted?: AudioDeleted | null;
  /** The latest draft's tooth chart confirmations, by tooth (ADR-0021 decision 10). */
  chart_checks?: ChartCheck[];
  /** Teeth with a drafted chart entry no veterinarian has confirmed; signing waits until it's empty. */
  unconfirmed_teeth?: string[];
  /** Set for a phone call through the clinic's own Twilio account (ADR-0023). */
  call?: CallInfo | null;
  /** A visit from another device (ADR-0017 section 7): the device that holds the audio. */
  audio_on?: { device_id: string; device_name: string } | null;
  /** Drafts set aside when this visit met a signature from another device, offered as addenda. */
  set_aside?: SetAsideRun[];
  /** Encrypted sync state of this visit. */
  sync?: SyncVisitState | null;
}

/** A draft set aside by sync (ADR-0017 section 9). */
export interface SetAsideRun {
  run_id: string;
  reason: "signed_elsewhere" | "signed_later";
  created_at: string;
  by: string | null;
  signed_at: string;
  signed_on: string | null;
  markdown: string | null;
}

/** A veterinarian's confirmation of the drafted chart entries on one tooth (ADR-0021 decision 10). */
export interface ChartCheck {
  tooth: string;
  /** The drafted entries confirmed (the engine's item keys). */
  items: string[];
  by: string;
  who?: Who | null;
  at: string;
}

// ---- deleting (ADR-0022) ----------------------------------------------------

/** When only the recording was deleted, and by whom. */
export interface AudioDeleted {
  at: string;
  by: Who;
}

export type ReasonKind = "test" | "mistake" | "duplicate" | "consent_withdrawn" | "other";

export interface DeleteReason {
  kind: ReasonKind;
  note?: string;
}

/** A visit just moved to Deleted visits. */
export interface DeletedVisit {
  trash_id: string;
  pets: string[];
  signed: boolean;
  purge_after: string;
  /** The main animal's patient record, to go back to. */
  patient_id: string | null;
}

/** A row of Settings, Deleted visits. */
export interface DeletedSummary {
  trash_id: string;
  /** "Olive Haddad"; empty when no patient was entered. */
  patient: string;
  patient_ids: string[];
  pets: string[];
  created_at: string;
  deleted_at: string;
  by: string;
  reason: DeleteReason;
  signed: boolean;
  purge_after: string;
  /** Phase 2: kept longer by the clinic's retention rule. */
  kept_until: string | null;
  bytes: number;
  recovered: boolean;
}

/** A visit brought back from Deleted visits (its id may have gained a suffix). */
export interface RestoredVisit {
  id: string;
  patient_id: string | null;
}

/** A document for the client (discharge instructions, a summary). */
export interface DocumentView {
  template_id: string;
  name: string;
  created_at: string;
  run: NoteRun | null;
  markdown: string | null;
  template: Template | null;
}

export interface Hardware {
  gpu: string | null;
  gpu_memory_bytes: number | null;
  ram_bytes: number;
  summary: string;
}

export interface Status {
  product: string;
  version: string;
  models_dir: string;
  sessions_dir: string;
  /** The models needed to start; `bundled`: came with the app (the iPhone app's speech models). */
  models: { id: string; name: string; installed: boolean; bundled?: boolean }[];
  hardware: Hardware;
  recording: boolean;
  /** Encryption at rest (ADR-0008). */
  encryption: { enabled: boolean; source: "environment" | "keychain" | "key_file" | null; warning: string | null };
  /** Encounters saved before encryption, still in plain files. */
  plain_encounters: number;
  /** Team members are set up (otherwise single-user). */
  team: boolean;
  /** "macos", "windows", "linux" or "ios": the Mac shows MLX-optimized models; a phone only mobile ones. */
  platform: string;
  /** Working alone, each person's own templates, or the hospital's (ADR-0024). */
  templates?: TemplatesMode;
  /** Which models this build offers: every one, or mobile ones only (ADR-0018 section 10). */
  profile?: "desktop" | "ios";
  /** A phone's memory tier in GB (6, 8 or 12); null on a computer or a phone too small to draft. */
  mobile_tier?: number | null;
  /** The note model unless the person picks another: Gemma 4 E4B on a computer, the tier's on a phone. */
  default_llm?: string;
}

export type Fit = "fits" | "tight" | "wont_fit";
export type Slot = "llm" | "stt" | "vad" | "diarization" | "speaker_embedding";

/** Where a model came from (ADR-0025). */
export type ModelOrigin = "builtin" | "hub" | "local_copy" | "local_in_place";
/**
 * What Remove does for a row: delete its files (the row stays), delete a copy
 * and drop the row, only forget it (used where it is), or nothing at all
 * (`needed` speech models; `with_gpu_pack` goes with the GPU speech pack).
 */
export type RowRemove = "delete" | "delete_and_forget" | "forget_only" | "needed" | "with_gpu_pack";

export interface ModelRow {
  id: string;
  name: string;
  publisher: string;
  slot: Slot;
  size_bytes: number;
  license: string;
  installed: boolean;
  fit: Fit;
  builtin: boolean;
  /** Came with the app (the iPhone app's speech models): Built in, nothing to download or remove. */
  bundled?: boolean;
  /** The credit the model's licence asks for, shown downloaded or built in. */
  attribution?: string;
  /**
   * Bytes on disk that are OpenBark's alone, `.part` files included: what
   * removing it frees (0 when used where it is).
   */
  disk_bytes: number;
  /**
   * Bytes on disk whose data another name also holds (a hard link: a copy
   * from this computer on the same drive). Removing them frees nothing.
   */
  shared_bytes: number;
  /** Bytes of a download that stopped part-way. */
  partial_bytes: number;
  origin: ModelOrigin;
  /** Note models only: which engine runs it. */
  engine: "llama" | "mlx" | null;
  /** Number of files (split GGUFs and MLX folders have several). */
  parts: number;
  remove: RowRemove;
  /** Used where it is, and the file or folder is gone. */
  missing: boolean;
  gated: boolean;
  /** Used where it is: the file or folder. */
  path: string | null;
  /** On a phone: why this model can't be downloaded or used here. */
  phone_note?: string;
}

/** What a removal did (ADR-0025 §10). */
export interface Removal {
  id: string;
  /** Space really freed. */
  freed_bytes: number;
  /** Bytes deleted whose data another name still holds (a hard link): nothing freed for them. */
  shared_bytes?: number;
  files_removed: number;
  /** Dropped from the list. */
  forgotten: boolean;
  /** Used where it is: nothing was deleted. */
  kept_in_place: boolean;
}

/** Who can run MLX models (ADR-0025 §1). */
export type MlxSupport = "yes" | "test_mode" | "needs_apple_silicon" | "needs_macos14" | "not_mac";

/** The MLX engine pack on this computer. */
export interface MlxStatus {
  support: MlxSupport;
  macos_version: string | null;
  pack_installed: boolean;
  /** Downloading or setting up right now. */
  installing: boolean;
  check: "not_run" | "passed" | "failed";
  check_message: string | null;
  metal: boolean;
  /** Still to download (0 when installed). */
  download_bytes: number;
  installed_bytes: number;
  versions: Record<string, string>;
  /** An MLX model is loaded now. */
  in_use: boolean;
}

export type LicenseClass = "permissive" | "restricted" | "unknown";

export interface HubModel {
  id: string;
  publisher: string;
  downloads: number;
  likes: number;
  license: string | null;
  license_class: LicenseClass;
  gated: boolean;
  last_modified: string | null;
  /** Converted for Apple's MLX framework (listed only on a Mac). */
  mlx: boolean;
}

/** Gated repos: can this computer's token see the files? (ADR-0025 §8) */
export type HubAccess = "open" | "needs_token" | "no_access_yet";

/** Why an MLX repo can't run. */
export type MlxProblem =
  | { kind: "no_config" }
  | { kind: "bad_config" }
  | { kind: "no_weights" }
  | { kind: "incomplete_shards"; missing: string[] }
  | { kind: "no_tokenizer" }
  | { kind: "hashes_hidden" };

/** The whole MLX model folder of a repo, offered as one row. */
export interface HubMlxView {
  size_bytes: number;
  files: number;
  fit: Fit;
  model_type: string | null;
  quantization: string | null;
  problem: MlxProblem | null;
  /** The MLX engine can run this `model_type`. */
  arch_supported: boolean;
  support: MlxSupport;
  pack_installed: boolean;
}

export interface HubRepoView {
  repo: {
    model: HubModel;
    revision: string;
    access: HubAccess;
    gated_kind: "auto" | "manual" | null;
    base_model: string | null;
  };
  /** GGUF choices; `parts` > 1 for a split GGUF. */
  files: { path: string; size_bytes: number; fit: Fit; parts: number }[];
  mlx: HubMlxView | null;
  /** On a phone: the files held back, and why. */
  held?: { path: string; size_bytes: number; reason: string }[];
  /** On a phone: one line when anything is held back. */
  phone_note?: string;
}

/** `hubAdd(repo, MLX_CHOICE, …)` adds an MLX repo's whole model folder. */
export const MLX_CHOICE = "@mlx";

export type ImportMode = "copy" | "in_place";

/** What adding a model from this computer would add, before anything changes. */
export interface ImportPreview {
  kind: "gguf" | "mlx";
  name: string;
  /** "local/<name>": also the `download` events' model while copying. */
  id: string;
  files: { path: string; size: number }[];
  total_bytes: number;
  model_type: string | null;
  /** A copy can be a link on the same drive (no extra space). */
  same_drive: boolean;
  free_bytes: number | null;
}

/** The saved Hugging Face token: never the token itself. */
export interface HfTokenStatus {
  saved: boolean;
  source: "server" | "keychain" | "vault_file" | "session" | null;
  /** The account name, when the token was saved in this run. */
  user: string | null;
}

// ---- templates ----------------------------------------------------------------

/** What goes under a heading: bullet points, named measurements, or medications. */
/** "text" is a text box: cited sentences shown as a paragraph. */
/** "teeth" is a tooth chart for dogs and cats (ADR-0021): one item per finding on one tooth. */
export type ContentKind = "statements" | "fields" | "medications" | "text" | "teeth";

export interface TemplateField {
  /** JSON key; the engine derives it from the label when empty. */
  key: string;
  label: string;
}

export interface TemplateCondition {
  species?: string[];
}

export interface TemplatePart {
  key: string;
  label: string;
  kind: ContentKind;
  presence?: string;
  instruction?: string | null;
  /** Example requests for this heading, shown as suggestions (never sent unless used). */
  examples?: string[];
  /** Shown only for these species ("show when species is feline"). */
  when?: TemplateCondition | null;
  fields?: TemplateField[];
  /** A tooth chart's dentition, set from the species or chosen when editing. */
  dentition?: "canine" | "feline" | null;
}

export interface TemplateSection {
  id: string;
  title: string;
  presence?: string;
  /** Content held directly under the heading; null/absent when it has subheadings. */
  kind?: ContentKind | null;
  instruction?: string | null;
  examples?: string[];
  when?: TemplateCondition | null;
  fields?: TemplateField[];
  parts?: TemplatePart[];
  /** A tooth chart's dentition, set from the species or chosen when editing. */
  dentition?: "canine" | "feline" | null;
}

/** A re-prompt saved against a heading, applied to every later draft. */
export interface Standard {
  id: string;
  /** "plan", "plan/medications", or "" for the whole note. */
  heading: string;
  text: string;
  added_at: string;
  added_by?: string | null;
  /** "team" (everyone, the default) or "personal" (only the person who saved it). */
  scope?: Scope;
}

export type Scope = "team" | "personal";

/** Where a template comes from (ADR-0024): shipped, the hospital's (team), or one person's own. */
export type Layer = "builtin" | "team" | "personal";

/** How templates work here: working alone, each person's own, or shared by the hospital. */
export type TemplatesMode = "alone" | "personal" | "hospital";

export interface BasedOn {
  layer: Layer;
  id: string;
  version: string;
  fingerprint: string;
}

/** Which version drafted a note, kept with the template and every run. */
export interface Origin {
  layer: Layer;
  owner?: Who | null;
  version: string;
  fingerprint: string;
  based_on?: BasedOn | null;
  seen_original?: string | null;
  saved_by?: Who | null;
  approved_by?: Who | null;
  saved_at?: string | null;
  object_id?: string | null;
}

export interface OriginalInfo {
  layer: Layer;
  id: string;
  name: string;
  version: string;
  fingerprint: string;
}

export type OriginalChange = "newer_builtin" | "team_changed" | "team_reset" | "team_deleted" | "no_longer_builtin";

export interface OriginalChanged {
  reason: OriginalChange;
  name: string;
  version: string;
  fingerprint: string;
}

export type LintReason =
  | "unsaid_findings"
  | "unsaid_numbers"
  | "unsaid_doses"
  | "removes_not_discussed"
  | "inventing"
  | "evidence";

/** A request that conflicts with the rules that always apply (a warning, never a block). */
export interface LintWarning {
  /** "instructions", "rules/2" or "headings/plan/medications". */
  path: string;
  text: string;
  reason: LintReason;
}

/** The "Always applied" panel. */
export interface AlwaysApplied {
  core: { text: string; summary: string }[];
  fixed: string[];
  checks: string[];
}

/** What the person looking may do with a template. */
export interface TemplateCan {
  edit: boolean;
  save_to: Layer | null;
  save_needs_pin: boolean;
  save_for_hospital: boolean;
  reset: boolean;
  share: boolean;
  reset_team: boolean;
  delete: boolean;
  delete_needs_pin: boolean;
  keep: boolean;
  keep_needs_pin: boolean;
  duplicate: boolean;
  change_document: boolean;
  standards_personal: boolean;
  standards_team_needs_pin: boolean;
  admin: boolean;
}

export type HeadingChange =
  | { kind: "added" }
  | { kind: "removed" }
  | { kind: "renamed"; from: string; to: string }
  | { kind: "moved"; from: number; to: number }
  | { kind: "kind_changed"; from: string; to: string }
  | { kind: "instruction_changed"; from: string | null; to: string | null }
  | { kind: "measurements_changed"; added: string[]; removed: string[]; renamed: [string, string][] }
  | { kind: "show_changed"; from: string[]; to: string[] }
  | { kind: "examples_changed"; added: string[]; removed: string[] };

/** A structured comparison of two templates (ADR-0024 section 9). */
export interface TemplateDiff {
  headings: { path: string; title: string; changes: HeadingChange[] }[];
  note: { field: string; from: unknown; to: unknown }[];
}

/** An admin's approval: their id and PIN, sent in a request body only. */
export interface Approver {
  id: string;
  pin: string;
}

export interface TeamSettingsView {
  templates: "personal" | "hospital";
  setup_notice: boolean;
}

export interface ProfileSummary {
  versions: number;
  own_templates: number;
  standards: number;
}

export interface Template {
  id: string;
  version: string;
  name: string;
  description?: string;
  /** "record" (default): the medical record. "client": for the pet owner. */
  document?: "record" | "client";
  /** Icon for this kind of visit (a Phosphor name, e.g. "stethoscope"). */
  icon?: string;
  /** How much the model writes. */
  detail?: "concise" | "standard" | "thorough";
  language?: string;
  instructions?: string;
  /** Safety rules from the built-in: shown, never edited (ADR-0024). */
  fixed_rules?: string[];
  rules?: string[];
  sections: TemplateSection[];
  standards?: Standard[];
  origin?: Origin | null;
}

export interface TemplateSummary {
  id: string;
  name: string;
  description: string;
  version: string;
  builtin: boolean;
  document: "record" | "client";
  icon?: string;
  headings: number;
  standards: number;
  /** The layer this person gets. */
  layer?: Layer;
  /** It changes an original (a built-in or the hospital's template). */
  edited?: boolean;
  /** Built in or the hospital's, as opposed to a template only this person uses. */
  shared?: boolean;
  original_changed?: OriginalChanged | null;
}

export interface TemplateView {
  template: Template;
  builtin: boolean;
  /** The system prompt drafts with this template start from. */
  prompt: string;
  layer: Layer;
  origin?: Origin | null;
  original?: OriginalInfo | null;
  original_changed?: OriginalChanged | null;
  has_base?: boolean;
  /** The hospital has its own version of this built-in. */
  team_version?: boolean;
  can: TemplateCan;
  always_applied: AlwaysApplied;
  lint: LintWarning[];
  not_used?: Standard[];
  muted?: string[];
  mode: TemplatesMode;
}

/** A reset, and the token its Undo sends. */
export interface ResetView {
  view: TemplateView;
  undo: string;
}

export interface Refinement {
  heading: string;
  request: string;
  from_run?: string | null;
  saved_as_standard: boolean;
}

/** "Plan > Medications" for a heading path, from a template. */
export function headingTitle(t: Template, heading: string): string {
  const [sec, part] = heading.split("/");
  const s = t.sections.find((x) => x.id === sec);
  if (!s) return heading;
  if (!part) return s.title;
  const p = s.parts?.find((x) => x.key === part);
  return p ? `${s.title} > ${p.label}` : s.title;
}

// ---- team members (M1 item 11) ------------------------------------------------

export type Role = "dvm" | "rvt" | "reception";

export interface User {
  id: string;
  name: string;
  role: Role;
  /** Changes the hospital's templates and the templates setting, with their PIN. */
  admin?: boolean;
}

/** Who did something, as stored on a revision. */
export interface Who {
  id?: string | null;
  name: string;
  role?: Role | null;
}

/**
 * Who is using the app: a team member's id, or (single-user) the name from
 * Settings. Sent with recordings, imports and drafts so each revision records
 * who made it. Set by the app shell.
 */
let actor: string | null = null;
export const setActor = (who: string | null) => {
  actor = who && who.trim() ? who : null;
};

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * The Tauri app on a phone or tablet (ADR-0018): there is no folder picker,
 * and the opener can only open web links.
 */
export const isPhoneShell =
  isDesktop && typeof navigator !== "undefined" && /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

/**
 * A phone: the app on one, or the phone screens in web mode
 * (`OPENBARK_PLATFORM=ios`, for demos and tests on a computer).
 */
export const isMobilePlatform = (platform?: string | null) =>
  isPhoneShell || platform === "ios" || platform === "android";

/**
 * Web mode: a request to this computer's OpenBark. Every request carries
 * `X-OpenBark`, which the server requires on anything that changes data, so
 * other web pages can't (os-server `local_only`).
 */
async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  headers.set("x-openbark", "1");
  const res = await fetch(path, { ...init, headers });
  // An older engine than these screens answers unknown API paths with the web page.
  if ((res.headers.get("content-type") ?? "").includes("text/html")) {
    throw new Error("OpenBark was updated but is still running the older version. Stop OpenBark and open it again.");
  }
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `${res.status} ${res.statusText}`);
  return body as T;
}

/**
 * POST a file, reporting bytes sent (fetch can't report upload progress).
 * Errors read like `api`'s.
 */
function uploadWithProgress<T>(path: string, body: Blob, onSent: (sent: number, total: number) => void): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("POST", path);
    xhr.setRequestHeader("content-type", body.type || "application/octet-stream");
    xhr.setRequestHeader("x-openbark", "1");
    xhr.upload.onprogress = (e) => onSent(e.loaded, e.lengthComputable ? e.total : body.size);
    xhr.onerror = () =>
      reject(new Error("OpenBark stopped responding. Check that it's still running, then try again."));
    xhr.onload = () => {
      let parsed: unknown = {};
      try {
        parsed = JSON.parse(xhr.responseText);
      } catch {
        // Not JSON: report the status below.
      }
      if (xhr.status >= 200 && xhr.status < 300) resolve(parsed as T);
      else reject(new Error((parsed as { error?: string }).error ?? `${xhr.status} ${xhr.statusText}`));
    };
    xhr.send(body);
  });
}

const post = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

const enc = encodeURIComponent;

export const status = () => (isDesktop ? invoke<Status>("status") : api<Status>("/api/status"));
export const inputDevices = () => (isDesktop ? invoke<string[]>("input_devices") : api<string[]>("/api/devices"));
// ---- storage (ADR-0013) ----------------------------------------------------

export type FolderKind = "models" | "visits";

export interface FolderView {
  kind: FolderKind;
  path: string;
  source: "default" | "chosen" | "fixed";
  /** What fixed it (an environment variable, or the command line). */
  fixed_by: string | null;
  used_bytes: number;
  free_bytes: number | null;
}

export interface StorageView {
  models: FolderView;
  visits: FolderView;
  /** Space Deleted visits take, inside the visits folder. */
  deleted_bytes?: number;
}

/** What choosing a folder would do, shown before anything changes. */
export interface FolderCheck {
  kind: FolderKind;
  chosen: string;
  /** The chosen folder, or an "OpenBark models/visits" folder inside it. */
  target: string;
  has_data: boolean;
  problem: FolderProblem | null;
  notices: FolderNotice[];
  needed_bytes: number;
  free_bytes: number | null;
  same_disk: boolean;
  can_move: boolean;
}

/** Why a chosen folder can't be used (translated on screen). */
export type FolderProblem =
  | { code: "fixed"; by: string }
  | { code: "not_absolute" }
  | { code: "same_folder" }
  | { code: "around_current" }
  | { code: "overlaps_other" }
  | { code: "inside_app_data" }
  | { code: "network_path" }
  | { code: "not_writable" };

/** Worth knowing about a chosen folder (translated on screen). */
export type FolderNotice = { code: "synced"; service: string } | { code: "network_drive" } | { code: "removable" };

export type ChangeMode = "move" | "switch";

export interface FolderChanged {
  kind: FolderKind;
  path: string;
  moved: { files: number; bytes: number; already_there: number } | null;
}

export interface FolderListing {
  path: string;
  parent: string | null;
  folders: { name: string; path: string }[];
}

export const storage = () => (isDesktop ? invoke<StorageView>("storage") : api<StorageView>("/api/storage"));

export const checkFolder = (kind: FolderKind, path: string) =>
  isDesktop
    ? invoke<FolderCheck>("check_folder", { kind, path })
    : api<FolderCheck>("/api/storage/check", post({ kind, path }));

export const changeFolder = (kind: FolderKind, path: string, mode: ChangeMode) =>
  isDesktop
    ? invoke<FolderChanged>("change_folder", { kind, path, mode })
    : api<FolderChanged>("/api/storage/change", post({ kind, path, mode }));

/**
 * Start OpenBark again, so new folders or newly installed libraries take
 * effect. Desktop restarts the app; web mode restarts the server and resolves
 * once it answers again (the caller reloads the page).
 */
export async function restartApp(): Promise<void> {
  if (isDesktop) return invoke<void>("restart_app");
  await api("/api/restart", { method: "POST" });
  await new Promise((r) => setTimeout(r, 800));
  for (let i = 0; i < 60; i++) {
    try {
      const res = await fetch("/api/status");
      if (res.ok) return;
    } catch {
      // Still starting.
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error("OpenBark didn't come back. Open it again from the Start menu.");
}

/** Desktop: the system's folder picker. Null if cancelled. */
export async function pickFolder(start?: string): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, defaultPath: start });
  return typeof picked === "string" ? picked : null;
}

/** Web mode: folders inside `path` (drives and the usual places when empty). */
export const listFolders = (path: string) => api<FolderListing>(`/api/folders?path=${enc(path)}`);

// ---- patients (ADR-0011) ---------------------------------------------------

export interface PatientRecord {
  id: string;
  name: string;
  last_name: string;
  species: string;
  created_at: string;
  updated_at?: string | null;
  /** The owner's phone number in E.164, for Call (ADR-0023). */
  owner_phone?: string | null;
}

export interface VisitSummary {
  id: string;
  created_at: string;
  reason: string;
  template: string;
  /** The template the latest draft used (empty before the first draft). */
  template_id: string;
  template_icon: string;
  drafted: boolean;
  signed: boolean;
  audio_secs: number;
  /** This animal's name, when the visit covered several. */
  pet?: string | null;
  source: string;
  /** The recording is still kept. */
  has_audio?: boolean;
}

export interface PatientSummary {
  patient: PatientRecord;
  visits: number;
  last_visit?: string | null;
  last_type: string;
  last_icon: string;
  last_reason: string;
}

export interface PatientView {
  patient: PatientRecord;
  visits: VisitSummary[];
  /** This patient's visits waiting in Deleted visits. */
  deleted?: number;
}

export const listPatients = () =>
  isDesktop ? invoke<PatientSummary[]>("list_patients") : api<PatientSummary[]>("/api/patients");
export const patientView = (id: string) =>
  isDesktop ? invoke<PatientView>("patient", { id }) : api<PatientView>(`/api/patients/${enc(id)}`);
export const createPatient = (name: string, lastName: string, species: string) =>
  isDesktop
    ? invoke<PatientRecord>("create_patient", { name, lastName, species })
    : api<PatientRecord>("/api/patients", post({ name, last_name: lastName, species }));
/** Change a patient's details. `ownerPhone`: empty clears it, `undefined` keeps it. */
export const updatePatientRecord = (id: string, name: string, lastName: string, species: string, ownerPhone?: string) =>
  isDesktop
    ? invoke<PatientRecord>("update_patient_record", { id, name, lastName, species, ownerPhone })
    : api<PatientRecord>(
        `/api/patients/${enc(id)}`,
        post({ name, last_name: lastName, species, ...(ownerPhone === undefined ? {} : { owner_phone: ownerPhone }) }),
      );

export const listSessions = () =>
  isDesktop ? invoke<SessionSummary[]>("list_sessions") : api<SessionSummary[]>("/api/sessions");
/** An encounter; `pet` picks one animal's notes in a multi-pet visit. */
export const openSession = (id: string, pet?: string | null) =>
  isDesktop
    ? invoke<SessionView>("open_session", { id, pet: pet ?? null })
    : api<SessionView>(`/api/sessions/${enc(id)}${pet ? `?pet=${enc(pet)}` : ""}`);
/** `?by=` for reads: the person using the app (ADR-0024: everyone gets their own version). */
const byQuery = () => (actor ? `?by=${enc(actor)}` : "");

export const listTemplates = () =>
  isDesktop
    ? invoke<TemplateSummary[]>("list_templates", { by: actor })
    : api<TemplateSummary[]>(`/api/templates${byQuery()}`);
export const template = (id: string) =>
  isDesktop
    ? invoke<TemplateView>("template", { id, by: actor })
    : api<TemplateView>(`/api/templates/${enc(id)}${byQuery()}`);
export const previewTemplate = (t: Template) =>
  isDesktop
    ? invoke<TemplateView>("preview_template", { template: t, by: actor })
    : api<TemplateView>("/api/templates/preview", post({ template: t, by: actor }));
/** A template proposed from an example note (not saved; M2 item 5). */
export const templateFromExample = (llm: string, text: string) =>
  isDesktop
    ? invoke<TemplateView>("template_from_example", { llm, text })
    : api<TemplateView>("/api/templates/from-example", post({ llm, text }));
export interface SaveTemplate {
  template: Template;
  /** The version and layer the editor started from: a different one is refused. */
  base_version?: string | null;
  base_layer?: Layer | null;
  /** Save to the hospital's templates (an admin, with their PIN). */
  hospital?: boolean;
  approver?: Approver | null;
}
export const saveTemplate = (req: SaveTemplate) => {
  const request = { ...req, by: actor };
  return isDesktop
    ? invoke<TemplateView>("save_template", { request })
    : api<TemplateView>("/api/templates", post(request));
};
/** "What changed in the original" (`base`) or "Yours and the original" (`original`). */
export const compareTemplate = (id: string, withWhat: "base" | "original") =>
  isDesktop
    ? invoke<TemplateDiff>("compare_template", { id, by: actor, with: withWhat })
    : api<TemplateDiff>(`/api/templates/${enc(id)}/compare?by=${enc(actor ?? "")}&with=${withWhat}`);
export const resetTemplate = (id: string) =>
  isDesktop
    ? invoke<ResetView>("reset_template", { id, by: actor })
    : api<ResetView>(`/api/templates/${enc(id)}/reset`, post({ by: actor }));
export const undoResetTemplate = (id: string, undo: string) =>
  isDesktop
    ? invoke<TemplateView>("undo_reset_template", { id, by: actor, undo })
    : api<TemplateView>(`/api/templates/${enc(id)}/reset/undo`, post({ by: actor, undo }));
export const keepTemplate = (id: string, fingerprint: string, approver?: Approver | null) =>
  isDesktop
    ? invoke<TemplateView>("keep_template", { id, by: actor, fingerprint, approver: approver ?? null })
    : api<TemplateView>(`/api/templates/${enc(id)}/keep`, post({ by: actor, fingerprint, approver: approver ?? null }));
export const shareTemplate = (id: string, mode: "replace" | "add", name: string | null, approver: Approver | null) =>
  isDesktop
    ? invoke<TemplateView>("share_template", { id, by: actor, mode, name, approver })
    : api<TemplateView>(`/api/templates/${enc(id)}/share`, post({ by: actor, mode, name, approver }));
export const resetTeamTemplate = (id: string, approver: Approver | null) =>
  isDesktop
    ? invoke<ResetView>("reset_team_template", { id, by: actor, approver })
    : api<ResetView>(`/api/templates/${enc(id)}/team/reset`, post({ by: actor, approver }));
export const deleteTemplate = (id: string, approver?: Approver | null) =>
  isDesktop
    ? invoke<void>("delete_template", { id, by: actor, approver: approver ?? null })
    : api<void>(`/api/templates/${enc(id)}/delete`, post({ by: actor, approver: approver ?? null }));
export const addStandard = (
  id: string,
  heading: string,
  text: string,
  by: string | null,
  scope?: Scope,
  approver?: Approver | null,
) =>
  isDesktop
    ? invoke<Standard>("add_standard", { id, heading, text, by, scope: scope ?? null, approver: approver ?? null })
    : api<Standard>(
        `/api/templates/${enc(id)}/standards`,
        post({ heading, text, by, scope: scope ?? null, approver: approver ?? null }),
      );
export const removeStandard = (id: string, sid: string, approver?: Approver | null) =>
  isDesktop
    ? invoke<TemplateView>("remove_standard", { id, sid, by: actor, approver: approver ?? null })
    : api<TemplateView>(
        `/api/templates/${enc(id)}/standards/${enc(sid)}/remove`,
        post({ by: actor, approver: approver ?? null }),
      );
/** Turn one of the hospital's standards off (or on) for yourself. */
export const muteStandard = (id: string, sid: string, muted: boolean) =>
  isDesktop
    ? invoke<TemplateView>("mute_standard", { id, sid, by: actor, muted })
    : api<TemplateView>(`/api/templates/${enc(id)}/standards/${enc(sid)}/mute`, post({ by: actor, muted }));
/** The template a visit's latest draft was written with (read-only). */
export const runTemplateView = (id: string, pet?: string | null) =>
  isDesktop
    ? invoke<TemplateView>("run_template_view", { id, pet: pet ?? null })
    : api<TemplateView>(`/api/sessions/${enc(id)}/template${pet ? `?pet=${enc(pet)}` : ""}`);
export const teamSettings = () =>
  isDesktop ? invoke<TeamSettingsView>("team_settings") : api<TeamSettingsView>("/api/team/settings");
export const setTeamSettings = (templates: "personal" | "hospital", approver: Approver | null) =>
  isDesktop
    ? invoke<TeamSettingsView>("set_team_settings", { templates, by: actor, approver })
    : api<TeamSettingsView>("/api/team/settings", post({ by: actor, templates, approver }));
export const dismissSetupNotice = () =>
  isDesktop
    ? invoke<TeamSettingsView>("dismiss_setup_notice", { by: actor })
    : api<TeamSettingsView>("/api/team/notice/dismiss", post({ by: actor }));
export const setUserAdmin = (uid: string, admin: boolean, approver: Approver | null) =>
  isDesktop
    ? invoke<User>("set_user_admin", { uid, admin, by: actor, approver })
    : api<User>(`/api/users/${enc(uid)}/admin`, post({ by: actor, admin, approver }));
/** What a team member keeps in their profile, before removing them. */
export const profileSummary = (uid: string) =>
  isDesktop
    ? invoke<ProfileSummary>("profile_summary", { uid, by: actor })
    : api<ProfileSummary>(`/api/users/${enc(uid)}/profile?by=${enc(actor ?? "")}`);
export interface RefineRequest {
  llm: string;
  heading: string;
  request: string;
  save_as_standard: boolean;
  by: string | null;
  /** Note language ("fr"…); English when absent. */
  language?: string | null;
  /** The animal, in a multi-pet visit. */
  pet?: string | null;
  /** Save the standard for yourself or for everyone (ADR-0024). */
  standard_scope?: Scope | null;
  /** For everyone, in a team: an admin's PIN. */
  approver?: Approver | null;
}
/** Redraft one heading of the latest draft; saved as a new draft. */
export const refineNote = (id: string, request: RefineRequest) =>
  isDesktop
    ? invoke<{ markdown: string; run: NoteRun }>("refine_note", { id, request })
    : api<{ markdown: string; run: NoteRun }>(`/api/sessions/${enc(id)}/refine`, post(request));
/** Draft with a template; `language` is the note language ("fr"…), English when absent. */
export const generateNote = (id: string, llm: string, tpl: string, language?: string | null, pet?: string | null) =>
  isDesktop
    ? invoke<{ markdown: string; run: NoteRun; notice?: string | null }>("generate_note", {
        id,
        llm,
        template: tpl,
        language: language ?? null,
        pet: pet ?? null,
        by: actor,
      })
    : api<{ markdown: string; run: NoteRun; notice?: string | null }>(
        `/api/sessions/${enc(id)}/notes`,
        post({ llm, template: tpl, language: language ?? null, pet: pet ?? null, by: actor }),
      );

/** Clinic letterhead and paper size for PDFs. */
export interface PdfRequest {
  clinic: { name: string; lines: string[] };
  page: "letter" | "a4";
  /** A client document's template id; the medical record when absent. */
  document?: string | null;
  /** Language for the PDF's own words ("Patient", "Page 1 of 2"). */
  language?: string | null;
  /** The animal, in a multi-pet visit. */
  pet?: string | null;
}

/** Save the note (or a client document) as a PDF. Resolves to false if cancelled. */
export async function exportPdf(id: string, request: PdfRequest): Promise<boolean> {
  if (isDesktop) {
    const name = await invoke<string>("pdf_name", { id, request });
    const path = await save({ defaultPath: name, filters: [{ name: "PDF", extensions: ["pdf"] }] });
    if (!path) return false;
    await invoke<void>("export_pdf", { id, path, request });
    return true;
  }
  // The server refuses a POST without X-OpenBark (os-server `local_only`).
  const res = await fetch(`/api/sessions/${enc(id)}/pdf`, {
    ...post(request),
    headers: { "content-type": "application/json", "x-openbark": "1" },
  });
  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as { error?: string };
    throw new Error(body.error ?? `${res.status} ${res.statusText}`);
  }
  const name = /filename="([^"]+)"/.exec(res.headers.get("content-disposition") ?? "")?.[1] ?? "note.pdf";
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return true;
}
export const updatePatient = (id: string, patient: Patient) =>
  isDesktop
    ? invoke<SessionView>("update_patient", { id, patient })
    : api<SessionView>(`/api/sessions/${enc(id)}/patient`, post(patient));
/**
 * Save the vet's own version of one heading as a new draft. `dentition`: the
 * animal chosen in the tooth chart editor when the chart had none.
 */
export const editNote = (id: string, heading: string, value: unknown, by: string, pet?: string | null, dentition?: "canine" | "feline" | null) =>
  isDesktop
    ? invoke<{ markdown: string; run: NoteRun }>("edit_note", { id, heading, value, by, pet: pet ?? null, dentition: dentition ?? null })
    : api<{ markdown: string; run: NoteRun }>(
        `/api/sessions/${enc(id)}/edit`,
        post({ heading, value, by, pet: pet ?? null, dentition: dentition ?? null }),
      );
export const addAddendum = (id: string, text: string, by: string, pet?: string | null) =>
  isDesktop
    ? invoke<SessionView>("add_addendum", { id, text, by, pet: pet ?? null })
    : api<SessionView>(`/api/sessions/${enc(id)}/addenda`, post({ text, by, pet: pet ?? null }));
/** Confirm (or take back) the drafted tooth chart entries on these teeth, as a veterinarian. */
export const confirmChart = (id: string, teeth: string[], confirmed: boolean, by: string, pet?: string | null) =>
  isDesktop
    ? invoke<SessionView>("confirm_chart", { id, teeth, confirmed, by, pet: pet ?? null })
    : api<SessionView>(`/api/sessions/${enc(id)}/chart/confirm`, post({ teeth, confirmed, by, pet: pet ?? null }));
export const signNote = (id: string, by: string, pet?: string | null) =>
  isDesktop
    ? invoke<SessionView>("sign_note", { id, by, pet: pet ?? null })
    : api<SessionView>(`/api/sessions/${enc(id)}/sign`, post({ by, pet: pet ?? null }));

// ---- deleting (ADR-0022) ----------------------------------------------------

/** Move a visit to Deleted visits for 30 days. */
export const deleteVisit = (id: string, by: string | null, reason: DeleteReason) => {
  const request = { by, reason: { kind: reason.kind, note: reason.note ?? "" } };
  return isDesktop
    ? invoke<DeletedVisit>("delete_visit", { id, request })
    : api<DeletedVisit>(`/api/sessions/${enc(id)}/delete`, post(request));
};
/** Delete only the recording; the transcript and notes stay. */
export const deleteRecording = (id: string, by: string | null) =>
  isDesktop
    ? invoke<SessionView>("delete_recording", { id, by })
    : api<SessionView>(`/api/sessions/${enc(id)}/recording/delete`, post({ by }));
export const listDeleted = (by: string | null) =>
  isDesktop
    ? invoke<DeletedSummary[]>("list_deleted", { by })
    : api<DeletedSummary[]>(`/api/deleted?by=${enc(by ?? "")}`);
export const restoreVisit = (tid: string, by: string | null) =>
  isDesktop
    ? invoke<RestoredVisit>("restore_visit", { tid, by })
    : api<RestoredVisit>(`/api/deleted/${enc(tid)}/restore`, post({ by }));
/** "Delete now": removed for good. A PIN (signed visits, phase 2) goes in the body only. */
export const purgeDeleted = (tid: string, by: string | null, pin?: string | null) =>
  isDesktop
    ? invoke<void>("purge_deleted", { tid, by, pin: pin ?? null })
    : api<void>(`/api/deleted/${enc(tid)}/purge`, post({ by, pin: pin ?? null }));
export const models = () => (isDesktop ? invoke<ModelRow[]>("models") : api<ModelRow[]>("/api/models"));
/** Speech recognition on the graphics card (ADR-0014). */
export interface GpuStatus {
  /** Windows on x64 with an NVIDIA graphics card. */
  supported: boolean;
  gpu: string | null;
  pack_installed: boolean;
  models_installed: boolean;
  /** Speech recognition runs on the GPU now. */
  active: boolean;
  /** Downloading or setting up right now. */
  installing: boolean;
  /** Installed since OpenBark started: restart to use it. */
  restart_needed: boolean;
  /** The self-test failed, so the processor is used. */
  check_failed: boolean;
  download_bytes: number;
  /** The pack's libraries plus the two full-precision models on disk. */
  installed_bytes: number;
}

export const gpuStatus = () => (isDesktop ? invoke<GpuStatus>("gpu_status") : api<GpuStatus>("/api/gpu"));
/** `acceptLicense`: the clinic ticked that it accepts NVIDIA's licence (required). */
export const installGpu = (acceptLicense: boolean) =>
  isDesktop
    ? invoke<GpuStatus>("install_gpu", { acceptLicense })
    : api<GpuStatus>("/api/gpu/install", post({ accept_license: acceptLicense }));
export const removeGpu = () =>
  isDesktop ? invoke<GpuStatus>("remove_gpu", { by: actor }) : api<GpuStatus>("/api/gpu/remove", post({ by: actor }));

// ---- removing and adding models (ADR-0025) ------------------------------------

/**
 * Remove a model's files, or forget one used where it is. `noteModel` is this
 * device's note model (a per-device setting), which the engine refuses to
 * remove; `forget` also drops a Hugging Face model from the list.
 */
export const removeModel = (id: string, noteModel: string | null, forget = false) =>
  isDesktop
    ? invoke<Removal>("remove_model", { id, noteModel, forget, by: actor })
    : api<Removal>("/api/models/remove", post({ id, note_model: noteModel, forget, by: actor }));

/** Look at a .gguf file or an MLX folder without changing anything (with a team, a veterinarian only). */
export const inspectImport = (path: string) =>
  isDesktop
    ? invoke<ImportPreview>("inspect_import", { path, by: actor })
    : api<ImportPreview>("/api/models/import/inspect", post({ path, by: actor }));

/** Add it: copied into the models folder, or used where it is. Progress comes as `download` events. */
export const importModel = (path: string, mode: ImportMode) =>
  isDesktop
    ? invoke<ModelRow>("import_model", { path, mode, by: actor })
    : api<ModelRow>("/api/models/import", post({ path, mode, by: actor }));

/** Desktop: the system's file picker for a .gguf model. Web: null (the sheet asks for a path). */
export async function pickModelFile(): Promise<string | null> {
  if (!isDesktop) return null;
  const picked = await open({ multiple: false, directory: false, filters: [{ name: "GGUF", extensions: ["gguf"] }] });
  return typeof picked === "string" ? picked : null;
}

/** Desktop: the system's folder picker for an MLX model folder. Web: null. */
export async function pickModelFolder(): Promise<string | null> {
  if (!isDesktop) return null;
  const picked = await open({ multiple: false, directory: true });
  return typeof picked === "string" ? picked : null;
}

// ---- MLX engine pack (ADR-0025) -------------------------------------------------

export const mlxStatus = () => (isDesktop ? invoke<MlxStatus>("mlx_status") : api<MlxStatus>("/api/mlx"));
/** Download and set up the pack; progress comes as `download` events for "mlx-engine-pack". */
export const installMlx = () =>
  isDesktop ? invoke<MlxStatus>("install_mlx") : api<MlxStatus>("/api/mlx/install", { method: "POST" });
/** Run the pack's check again ("Try again"). */
export const checkMlx = () =>
  isDesktop ? invoke<MlxStatus>("check_mlx") : api<MlxStatus>("/api/mlx/check", { method: "POST" });
export const removeMlx = (noteModel: string | null) =>
  isDesktop
    ? invoke<Removal>("remove_mlx", { noteModel, by: actor })
    : api<Removal>("/api/mlx/remove", post({ note_model: noteModel, by: actor }));
/** The `download` events' model while the MLX engine pack downloads. */
export const MLX_PACK_ID = "mlx-engine-pack";

// ---- Hugging Face token (ADR-0025 §8) ---------------------------------------------

export const hfTokenStatus = () =>
  isDesktop ? invoke<HfTokenStatus>("hf_token_status") : api<HfTokenStatus>("/api/hf-token");
/** Checked with Hugging Face first (one call), then saved. The token never comes back. */
export const setHfToken = (token: string) =>
  isDesktop
    ? invoke<HfTokenStatus>("set_hf_token", { token, by: actor })
    : api<HfTokenStatus>("/api/hf-token", post({ token, by: actor }));
export const clearHfToken = () =>
  isDesktop
    ? invoke<HfTokenStatus>("clear_hf_token", { by: actor })
    : api<HfTokenStatus>("/api/hf-token/clear", post({ by: actor }));

/** GGUF builds of an MLX repo's base model, to suggest instead. */
export const hubAlternatives = (base: string) =>
  isDesktop
    ? invoke<HubModel[]>("hub_alternatives", { base })
    : api<HubModel[]>(`/api/hub/alternatives?base=${enc(base)}`);

type Translate = (text: string, vars?: Record<string, string | number>) => string;

/**
 * The engine's messages about models (ADR-0025, contracts 3 to 6 and 10), as
 * `t()` calls so scripts/i18n-keys.py lists them for translation. Messages
 * with a name, path or size in them are matched by pattern below.
 */
export const MODEL_MESSAGES = (t: Translate) => [
  t("Only a veterinarian can change the models."),
  t("No model called {id} in the catalog."),
  t("OpenBark needs this to transcribe visits."),
  t("This comes with the GPU speech pack. Remove the pack instead."),
  t("This model is still downloading."),
  t("This model writes your notes. Choose another one under In use first."),
  t("A note is being written with this model. Remove it when that's finished."),
  t("Your note model uses the MLX engine. Choose another one under In use first."),
  t("A note is being written with an MLX model. Remove the engine when that's finished."),
  t("Download the MLX engine on the Models screen first."),
  t("The MLX engine didn't pass its check on this computer. Choose Try again on the MLX engine card in Models."),
  t("MLX models run on a Mac with Apple silicon and macOS 14 or later."),
  t("The MLX engine is already downloading."),
  t("{name} isn't where it was ({path}). Add it again from this computer."),
  t("{name} didn't pass OpenBark's check, so it can't write notes."),
  t("This model is gated on Hugging Face. Request access on its page, then add your token."),
  t("Hugging Face couldn't find {url}. It may have been removed or made private."),
  t("{0} was added from this computer, so it can't be downloaded."),
  t("{0} wasn't found on Hugging Face, or it's private."),
  t("Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens."),
  t("OpenBark can't find {0}."),
  t("Give the full path, starting from the top of the drive."),
  t("That isn't a GGUF model file."),
  t("That's a helper file ({0}), not a model. Choose the main model file."),
  t("Some parts of this model are missing from the folder: {0}."),
  t("That folder isn't an MLX model OpenBark can run: {0}"),
  t("{0} is a link to a file outside the folder you chose, so OpenBark won't add it."),
  t("An older version of this model is on this computer. Remove it on the Models screen, then add it again."),
  t("Not enough space: this needs {need} bytes and {available} are free."),
  t("OpenBark won't delete {0}: it's outside the models folder."),
  t("OpenBark won't delete {0}: it's a link to somewhere else."),
];

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
let messagePatterns: { english: string; re: RegExp; names: string[] }[] | null = null;

/** An engine message about models in the app language (English when unknown). */
export function modelMessage(t: Translate, e: unknown): string {
  const text = e instanceof Error ? e.message : String(e);
  messagePatterns ??= MODEL_MESSAGES((s) => s).map((english) => {
    const names = [...english.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "");
    const re = new RegExp(
      `^${english
        .split(/\{\w+\}/)
        .map(escapeRe)
        .join("(.+?)")}$`,
      "s",
    );
    return { english, re, names };
  });
  for (const { english, re, names } of messagePatterns) {
    const m = text.match(re);
    if (!m) continue;
    if (names.length === 0) return t(english);
    return t(english, Object.fromEntries(names.map((n, i) => [n, m[i + 1] ?? ""])));
  }
  return text;
}

export const downloadModel = (id: string) =>
  isDesktop ? invoke<void>("download_model", { id }) : api<void>(`/api/models/${enc(id)}/download`, { method: "POST" });
export const hubSearch = (q: string) =>
  isDesktop ? invoke<HubModel[]>("hub_search", { q }) : api<HubModel[]>(`/api/hub/search?q=${enc(q)}`);
export const hubRepo = (id: string) =>
  isDesktop ? invoke<HubRepoView>("hub_repo", { id }) : api<HubRepoView>(`/api/hub/repo?id=${enc(id)}`);
export const hubAdd = (repo: string, file: string, acceptLicense: boolean) =>
  isDesktop
    ? invoke<ModelRow>("hub_add", { repo, file, acceptLicense })
    : api<ModelRow>("/api/hub/add", post({ repo, file, accept_license: acceptLicense }));

/**
 * WebKitGTK (the Linux desktop app) won't play media from an app-defined
 * scheme at all (media error 4 before any request), so there the player
 * starts from the fallback.
 */
const linuxWebview = () => typeof navigator !== "undefined" && /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);

/**
 * A URL the player can use for the recording. Audio is encrypted on disk
 * (ADR-0008), so it's decrypted as it's played, in byte ranges: by the
 * server in web mode, and by the desktop app's `obaudio` scheme
 * (`obaudio://localhost/<id>` on macOS, `http://obaudio.localhost/<id>` on
 * Windows). On the Linux desktop it's a blob: URL from
 * {@link audioFallbackUrl}; revoke blob: URLs when done.
 */
export async function audioUrl(s: SessionView): Promise<string> {
  if (!isDesktop) return `/api/sessions/${enc(s.id)}/audio`;
  if (linuxWebview()) return (await audioFallbackUrl(s)) ?? convertFileSrc(s.id, "obaudio");
  return convertFileSrc(s.id, "obaudio");
}

/**
 * The desktop player's fallback when its URL won't play: the whole decrypted
 * recording, from memory (a blob: URL). Revoke it when done. Null in web mode,
 * where there's nothing else to try.
 */
export async function audioFallbackUrl(s: SessionView): Promise<string | null> {
  if (!isDesktop) return null;
  const bytes = await invoke<ArrayBuffer>("session_audio", { id: s.id });
  return URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
}

export const listUsers = () => (isDesktop ? invoke<User[]>("list_users") : api<User[]>("/api/users"));
export const addUser = (name: string, role: Role, pin: string, by: string | null) =>
  isDesktop
    ? invoke<User>("add_user", { name, role, pin, by })
    : api<User>("/api/users", post({ name, role, pin, by }));
export const removeUser = (uid: string, by: string | null) =>
  isDesktop
    ? invoke<void>("remove_user", { uid, by })
    : api<void>(`/api/users/${enc(uid)}?by=${enc(by ?? "")}`, { method: "DELETE" });
export const setUserPin = (uid: string, pin: string, by: string | null) =>
  isDesktop
    ? invoke<void>("set_user_pin", { uid, pin, by })
    : api<void>(`/api/users/${enc(uid)}/pin`, post({ pin, by }));
export const verifyUser = (uid: string, pin: string) =>
  isDesktop ? invoke<User>("verify_user", { uid, pin }) : api<User>(`/api/users/${enc(uid)}/verify`, post({ pin }));

export const encryptExisting = () =>
  isDesktop ? invoke<number>("encrypt_existing") : api<number>("/api/encryption/encrypt-existing", { method: "POST" });
// ---- encrypted sync (ADR-0017) ---------------------------------------------
// Rust makes every sync call (the desktop app's commands, or `openbark serve`
// in web mode). This screen never sees a key, a token or the server.

export type SyncPhase = "off" | "code_sent" | "pending" | "kit_check" | "active" | "signed_out" | "revoked";

export interface SyncDevice {
  id: string;
  name: string;
  platform: string;
  status: "approved" | "revoked" | "pending";
  this_device: boolean;
  last_seen_at: string | null;
  approved_at: string | null;
}

export interface SyncConflict {
  visit: string;
  field: string;
  kept: unknown;
  other: unknown;
  kept_from: string;
  other_from: string;
  at: string;
}

export interface SyncPairing {
  words: string[];
  qr_svg: string;
  expires_at: string | null;
}

export interface SyncKit {
  words: string[];
  qr_svg: string;
  /** Which two words (1-based) to type back. */
  check: [number, number];
}

export interface SyncStatus {
  available: boolean;
  phase: SyncPhase;
  email?: string | null;
  server?: string;
  custom_server?: boolean;
  device_id?: string;
  device_name?: string;
  devices?: SyncDevice[];
  last_sync_at?: string | null;
  last_error?: string | null;
  pending_uploads?: number;
  conflicts?: SyncConflict[];
  warnings?: string[];
  pairing?: SyncPairing | null;
  kit?: SyncKit | null;
  kit_check?: [number, number] | null;
  keys_in?: string;
  busy?: boolean;
}

export type SyncNext =
  | ({ next: "kit" } & SyncKit)
  | ({ next: "pair" } & SyncPairing)
  | { next: "active" };

export interface SyncPending {
  device_id: string;
  name: string;
  platform: string;
}

export const syncStatus = () => (isDesktop ? invoke<SyncStatus>("sync_status") : api<SyncStatus>("/api/sync"));
export const syncSetServer = (url: string, key: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_set_server", { url, key, by: actor })
    : api<SyncStatus>("/api/sync/server", post({ url, key, by: actor }));
export const syncSendCode = (email: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_send_code", { email, by: actor })
    : api<SyncStatus>("/api/sync/code", post({ email, by: actor }));
/** `backfillDays` null uploads every visit on this computer. */
export const syncVerify = (code: string, deviceName: string, backfillDays: number | null) =>
  isDesktop
    ? invoke<SyncNext>("sync_verify", { code, deviceName, backfillDays, by: actor })
    : api<SyncNext>("/api/sync/verify", post({ code, device_name: deviceName, backfill_days: backfillDays, by: actor }));
export const syncConfirmKit = (answers: [number, string][]) =>
  isDesktop
    ? invoke<SyncStatus>("sync_confirm_kit", { answers, by: actor })
    : api<SyncStatus>("/api/sync/kit/confirm", post({ answers, by: actor }));
export const syncNewCode = () =>
  isDesktop ? invoke<SyncPairing>("sync_new_code") : api<SyncPairing>("/api/sync/pair/new", post({}));
/** Find the waiting device whose words were typed; with `approve`, add it. */
export const syncPair = (words: string, approve: boolean) =>
  isDesktop
    ? invoke<SyncPending>("sync_pair", { words, approve, by: actor })
    : api<SyncPending>("/api/sync/pair", post({ words, approve, by: actor }));
export const syncRecover = (words: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_recover", { words, by: actor })
    : api<SyncStatus>("/api/sync/recover", post({ words, by: actor }));
export const syncNow = () => (isDesktop ? invoke<SyncStatus>("sync_now") : api<SyncStatus>("/api/sync/now", post({})));
export const syncRemoveDevice = (deviceId: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_remove_device", { deviceId, by: actor })
    : api<SyncStatus>(`/api/sync/devices/${enc(deviceId)}/remove`, post({ by: actor }));
export const syncOff = () =>
  isDesktop ? invoke<SyncStatus>("sync_off", { by: actor }) : api<SyncStatus>("/api/sync/off", post({ by: actor }));
export const syncDeleteAccount = (confirm: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_delete_account", { confirm, by: actor })
    : api<SyncStatus>("/api/sync/delete-account", post({ confirm, by: actor }));
export const syncStopVisit = (id: string) =>
  isDesktop
    ? invoke<SyncStatus>("sync_stop_visit", { id, by: actor })
    : api<SyncStatus>(`/api/sync/visits/${enc(id)}/stop`, post({ by: actor }));

export const recoveryKey = () =>
  isDesktop ? invoke<string>("recovery_key") : api<string>("/api/encryption/recovery-key");

/** Show files: desktop only (the opener can only open web links on a phone). */
export const canReveal = isDesktop && !isPhoneShell;
export const reveal = (path: string) => revealItemInDir(path);

// ---- importing a recording file -------------------------------------------

/**
 * Recordings the engine decodes: WAV (PCM, ADPCM), AIFF, MP3 and MP2, phone
 * recordings (m4a, caf, mov), FLAC, Ogg and Opus voice notes, WebM and MKV
 * from browsers and call apps, and video files (their audio track).
 */
export const AUDIO_EXTENSIONS = [
  "wav",
  "m4a",
  "mp4",
  "m4v",
  "mov",
  "aac",
  "caf",
  "mp3",
  "mp2",
  "flac",
  "ogg",
  "oga",
  "opus",
  "webm",
  "weba",
  "mkv",
  "mka",
  "aif",
  "aiff",
  "aifc",
];

/**
 * Web mode: decode a recording the engine can't read, in the browser (which
 * knows more codecs, such as HE-AAC), and hand back a 16 kHz mono WAV. Null
 * when the browser can't read it either.
 */
export async function decodeInBrowser(file: Blob): Promise<Blob | null> {
  const rate = 16000;
  try {
    // decodeAudioData resamples to the context's rate.
    const ctx = new OfflineAudioContext(1, 1, rate);
    const audio = await ctx.decodeAudioData(await file.arrayBuffer());
    const mono = new Float32Array(audio.length);
    for (let c = 0; c < audio.numberOfChannels; c++) {
      const data = audio.getChannelData(c);
      for (let i = 0; i < data.length; i++) mono[i]! += data[i]! / audio.numberOfChannels;
    }
    return wavBlob(mono, rate);
  } catch {
    return null;
  }
}

/** A 16-bit PCM mono WAV. */
export function wavBlob(samples: Float32Array, rate: number): Blob {
  const bytes = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(bytes);
  const text = (at: number, s: string) => [...s].forEach((ch, i) => v.setUint8(at + i, ch.charCodeAt(0)));
  text(0, "RIFF");
  v.setUint32(4, 36 + samples.length * 2, true);
  text(8, "WAVEfmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  text(36, "data");
  v.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]!));
    v.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Blob([bytes], { type: "audio/wav" });
}

function pickBrowserFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = AUDIO_EXTENSIONS.map((e) => `.${e}`).join(",") + ",audio/*";
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

/** Ask for a WAV, then create an encounter from it. Null if cancelled. */
/**
 * Steps of turning audio into a saved transcript, for a progress bar. The
 * engine reports `read`, `transcribe` (seconds of audio), `check` (segments)
 * and `save`; web mode adds `upload` (bytes) and `convert` (the browser
 * decoding a format the engine can't read).
 */
export type ProcessingStage =
  | "upload"
  | "convert"
  | "read"
  | "transcribe"
  | "check"
  | "save"
  // Drafting a note (the drafting board in Review).
  | "load_model"
  | "read_transcript"
  | "write"
  | "verify";
export interface ProcessingProgress {
  stage: ProcessingStage;
  done: number;
  /** 0 when not known (a stopped recording finishing its last words). */
  total: number;
}

export async function importRecording(
  speakers: boolean,
  speakerphone: boolean,
  patient: Patient | null,
  consent: RecordingConsent | null,
  onPicked: () => void,
  onProgress: (p: ProcessingProgress) => void = () => {},
): Promise<SessionView | null> {
  const p = hasPatient(patient) ? patient : null;
  if (isDesktop) {
    const picked = await open({ multiple: false, filters: [{ name: "Recording", extensions: AUDIO_EXTENSIONS }] });
    if (typeof picked !== "string") return null;
    onPicked();
    return invoke<SessionView>("import_file", { path: picked, speakers, speakerphone, patient: p, consent, by: actor });
  }
  const file = await pickBrowserFile();
  if (!file) return null;
  onPicked();
  const upload = (body: Blob, name: string) => {
    const q = new URLSearchParams({
      diarize: String(speakers),
      speakerphone: String(speakerphone),
      name,
      patient_name: p?.name ?? "",
      others: (p?.others ?? [])
        .map((o) => o.trim())
        .filter(Boolean)
        .join("\n"),
      species: p?.species ?? "",
      patient_id: p?.patient_id ?? "",
      last_name: p?.last_name ?? "",
      reason: p?.reason ?? "",
      ...consentQuery(consent),
      by: actor ?? "",
    });
    return uploadWithProgress<SessionView>(`/api/sessions?${q}`, body, (done, total) =>
      onProgress({ stage: "upload", done, total }),
    );
  };
  try {
    return await upload(file, file.name);
  } catch (e) {
    // The engine can't read this kind of file: try the browser's decoders.
    if (!String(e instanceof Error ? e.message : e).includes("can't read")) throw e;
    onProgress({ stage: "convert", done: 0, total: 1 });
    const wav = await decodeInBrowser(file);
    if (!wav) throw e;
    return upload(wav, `${file.name.replace(/\.[^.]+$/, "")}.wav`);
  }
}

// ---- live recording ---------------------------------------------------------

export interface Recording {
  /** Resolves to the saved encounter once the transcript is complete. */
  stop(): Promise<SessionView>;
  /** Web mode: the microphone the browser opened. */
  mic?: { label: string; sampleRate: number; fellBack: boolean };
}

/** Desktop: the engine records the microphone natively (levels arrive as events). */
async function startDesktopRecording(
  speakers: boolean,
  speakerphone: boolean,
  device: string | null,
  patient: Patient | null,
  consent: RecordingConsent | null,
): Promise<Recording> {
  await invoke<string>("start_recording", { device, speakers, speakerphone, patient, consent, by: actor });
  return { stop: () => invoke<SessionView>("stop_recording") };
}

/**
 * Web: the browser captures the user's own microphone and streams PCM to the
 * engine over a WebSocket (mic/browser.ts). Levels are measured there so the
 * meter reacts to the real input with no round trip.
 */
async function startBrowserRecording(
  speakers: boolean,
  speakerphone: boolean,
  deviceId: string | null,
  patient: Patient | null,
  consent: RecordingConsent | null,
  hooks: RecordingHooks,
): Promise<Recording> {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const connect = (sampleRate: number) => {
    const q = new URLSearchParams({
      speakers: String(speakers),
      speakerphone: String(speakerphone),
      sample_rate: String(sampleRate),
      patient_name: patient?.name ?? "",
      others: (patient?.others ?? [])
        .map((o) => o.trim())
        .filter(Boolean)
        .join("\n"),
      species: patient?.species ?? "",
      patient_id: patient?.patient_id ?? "",
      last_name: patient?.last_name ?? "",
      reason: patient?.reason ?? "",
      ...consentQuery(consent),
      by: actor ?? "",
    });
    return new WebSocket(`${proto}://${location.host}/api/live?${q}`);
  };
  const rec = await recordInBrowser({
    deviceId,
    connect,
    onLevel: hooks.onLevel,
    onHealth: (h) => hooks.onHealth?.(h),
    onEnded: (m) => hooks.onEnded?.(m),
  });
  return { stop: () => rec.stop() as Promise<SessionView>, mic: rec.mic };
}

export interface RecordingHooks {
  onLevel(level: number, seconds: number): void;
  /** Web mode: the browser's own view of its microphone (desktop: the "mic" event). */
  onHealth?(health: MicHealth): void;
  /** Web mode: the recording ended by itself (the engine went away). */
  onEnded?(message: string): void;
}

/**
 * Start recording; `consent` is saved with the encounter when the consent
 * step was used. `device` is the desktop microphone's name, or in web mode
 * the browser's device id; null is the system default.
 */
export type { MicHealth, MicQuality };

export function startRecording(
  speakers: boolean,
  speakerphone: boolean,
  device: string | null,
  patient: Patient | null,
  consent: RecordingConsent | null,
  hooks: RecordingHooks | ((level: number, seconds: number) => void),
): Promise<Recording> {
  const p = hasPatient(patient) ? patient : null;
  const h = typeof hooks === "function" ? { onLevel: hooks } : hooks;
  return isDesktop
    ? startDesktopRecording(speakers, speakerphone, device, p, consent)
    : startBrowserRecording(speakers, speakerphone, device, p, consent, h);
}

/**
 * The Record screen opened: load the speech models now (if memory allows),
 * so Record starts at once. Best effort: false when it didn't.
 */
export const prewarmSpeech = (speakers: boolean, speakerphone = false): Promise<boolean> =>
  (isDesktop
    ? invoke<boolean>("prewarm_speech", { speakers, speakerphone })
    : api<{ loading: boolean }>("/api/record/prewarm", post({ speakers, speakerphone })).then((r) => r.loading)
  ).catch(() => false);

/** The Record screen closed: give the memory back. */
export const releaseSpeech = (): Promise<void> =>
  (isDesktop ? invoke<void>("release_speech") : api<unknown>("/api/record/release", post({})).then(() => undefined)).catch(
    () => undefined,
  );

/** The microphone a desktop test opened. */
export interface MicTestStarted {
  /** This test's number: its "micTest" events carry it. */
  test: number;
  device: string;
  sample_rate: number;
  quality: MicQuality;
  /** The chosen microphone wasn't connected; this is the default. */
  replaced: string | null;
}

/** The desktop microphone test's level (the "micTest" event), about 20 times a second. */
export interface MicLevel {
  /** Which test sent it (a stopped test's last events can still arrive). */
  test: number;
  device: string;
  sample_rate: number;
  quality: MicQuality;
  level: number;
  silent: boolean;
  done: boolean;
}

/** Desktop: test a microphone (levels arrive as "micTest" events). */
export const micTestStart = (device: string | null) => invoke<MicTestStarted>("mic_test_start", { device });
/** Stop the test with this number (or whichever is running). */
export const micTestStop = (test?: number) => invoke<void>("mic_test_stop", { test: test ?? null });

const METHOD_TEXT: Record<ConsentMethod, string> = {
  verbal: "agreed out loud",
  written: "signed a consent form",
  on_file: "consent on file",
  clinician_only: "clinician consent only (one-party rule)",
  announced: "heard the recorded announcement",
};

/** "Client agreed out loud; confirmed by Dr Patel, 2026-10-17 20:07". */
export function consentText(c: RecordingConsent): string {
  const how = c.method === "clinician_only" ? METHOD_TEXT[c.method] : `Client ${METHOD_TEXT[c.method]}`;
  return `${how[0]?.toUpperCase()}${how.slice(1)}; confirmed by ${c.confirmed_by}${c.confirmed_at ? `, ${fmtStamp(c.confirmed_at)}` : ""}`;
}

/** "2026-10-17 19:20" from an engine timestamp. */
export function fmtStamp(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

// ---- phone calls (ADR-0023) ------------------------------------------------
// The clinic's own Twilio account. The main window only connects, sets up and
// says who to call; the call itself runs in the separate phone window
// (phone.html), the only page that may reach Twilio.

export interface PhonePrefs {
  /** "Turn on the phone on this computer". */
  enabled: boolean;
  record_calls: boolean;
  announcement: string;
}

export interface SecretStatus {
  location: "keychain" | "sealed_file";
  /** "****4f2a". */
  hint: string;
  warning?: string | null;
}

export interface TwilioView {
  account_sid: string;
  api_key_hint: string;
  api_key_sid: string;
  computer_name: string;
  identity: string;
  caller_id?: string | null;
  outgoing_bin_url?: string | null;
  twiml_app_sid?: string | null;
}

export interface PhoneView {
  connected: boolean;
  twilio: TwilioView | null;
  secret: SecretStatus | null;
  prefs: PhonePrefs;
  /** Calls can be placed now. */
  ready: boolean;
  /** The Linux desktop app can't make calls (web mode can). */
  linux: boolean;
  outgoing_bin: string | null;
}

export interface ConnectTwilio {
  account_sid: string;
  api_key_sid: string;
  /** Empty keeps the secret already saved for the same key. */
  api_key_secret: string;
  computer_name: string;
  caller_id: string;
  outgoing_bin_url: string;
}

export interface SetupCheck {
  id: "key" | "caller_id" | "twiml_app" | "phone_on" | "trial";
  ok: boolean | null;
  message: string;
}

export interface CallTarget {
  patient_id: string | null;
  patient_name: string;
  last_name: string;
  number: string | null;
  record_calls: boolean;
  announcement: string;
  by: string | null;
  clinic_label: string;
}

export const phone = () => (isDesktop ? invoke<PhoneView>("phone") : api<PhoneView>("/api/phone"));
/** The outgoing Bin's text for a caller ID (no network). */
export const phoneBin = (callerId: string) =>
  isDesktop
    ? invoke<string>("phone_bin", { callerId })
    : api<{ text: string }>(`/api/phone/bin?caller_id=${enc(callerId)}`).then((r) => r.text);
/** Connect Twilio. The secret goes to the engine, which sends it only to Twilio. */
export const phoneConnect = (request: ConnectTwilio) =>
  isDesktop
    ? invoke<{ phone: PhoneView }>("phone_connect", { request, by: actor })
    : api<{ phone: PhoneView }>("/api/phone/connect", post({ ...request, by: actor }));
export const phoneDisconnect = () =>
  isDesktop
    ? invoke<{ note: string }>("phone_disconnect", { by: actor })
    : api<{ note: string }>("/api/phone/disconnect", post({ by: actor }));
export const phoneSetPrefs = (prefs: PhonePrefs) =>
  isDesktop
    ? invoke<PhoneView>("phone_set_prefs", { prefs, by: actor })
    : api<PhoneView>("/api/phone/prefs", post({ ...prefs, by: actor }));
export const phoneCheck = () =>
  isDesktop
    ? invoke<SetupCheck[]>("phone_check", { by: actor })
    : api<SetupCheck[]>("/api/phone/check", post({ by: actor }));

/** The phone page's channel to the main window in web mode (same origin). */
export const PHONE_CHANNEL = "openbark-phone";

/**
 * **Call**: tells the engine who to call, then opens the phone window. On the
 * desktop that's a second Tauri window; in web mode, a pop-up of this page.
 * Returns false when the browser blocked the pop-up.
 */
export async function openPhone(call: { patient_id?: string | null; number?: string | null }): Promise<boolean> {
  if (isDesktop) {
    await invoke<CallTarget>("phone_open", { call, by: actor });
    return true;
  }
  await api<CallTarget>("/api/phone/prepare", post({ ...call, by: actor }));
  try {
    new BroadcastChannel(PHONE_CHANNEL).postMessage({ type: "target" });
  } catch {
    // Old browsers: the phone page reads the call when it opens.
  }
  const w = window.open("/phone.html", "openbark-phone", "popup,width=420,height=760");
  if (!w) return false;
  // The phone page can't reach back into this window: no opener, as with
  // noopener (which would hide whether the pop-up was blocked).
  w.opener = null;
  w.focus();
  return true;
}

/** Called with a visit's id when a recorded call is saved (from the phone window). */
export function onCallSaved(cb: (id: string) => void): () => void {
  if (isDesktop) {
    const off = listen<{ id: string }>("phone-call-saved", (e) => cb(e.payload.id));
    return () => void off.then((f) => f());
  }
  let channel: BroadcastChannel | null = null;
  try {
    channel = new BroadcastChannel(PHONE_CHANNEL);
    channel.onmessage = (e: MessageEvent<{ type?: string; id?: string }>) => {
      if (e.data?.type === "saved" && e.data.id) cb(e.data.id);
    };
  } catch {
    // No BroadcastChannel: the visit still appears under Patients.
  }
  return () => channel?.close();
}

// ---- events -----------------------------------------------------------------

type Unlisten = () => void;
type Handlers = {
  /** The recording's microphone health (desktop; web mode reports its own). */
  mic: Set<(h: MicHealth) => void>;
  /** The desktop microphone test's level. */
  micTest: Set<(l: MicLevel) => void>;
  segment: Set<(s: Segment) => void>;
  token: Set<(t: string) => void>;
  level: Set<(l: { level: number; seconds: number }) => void>;
  download: Set<(d: { model: string; done: number; total: number }) => void>;
  progress: Set<(p: ProcessingProgress) => void>;
  moving: Set<(m: { done: number; total: number }) => void>;
};
const handlers: Handlers = {
  mic: new Set(),
  micTest: new Set(),
  segment: new Set(),
  token: new Set(),
  level: new Set(),
  download: new Set(),
  progress: new Set(),
  moving: new Set(),
};
let source: EventSource | null = null;

function sse() {
  if (source || isDesktop) return;
  source = new EventSource("/api/events");
  source.onmessage = (e) => {
    const msg = JSON.parse(e.data) as { type: keyof Handlers; data: never };
    handlers[msg.type]?.forEach((h) => (h as (d: never) => void)(msg.data));
  };
}

const TAURI_EVENT: Record<keyof Handlers, string> = {
  mic: "mic-status",
  micTest: "mic-test",
  segment: "transcript-segment",
  token: "note-token",
  level: "audio-level",
  download: "download-progress",
  progress: "processing-progress",
  moving: "move-progress",
};

export async function on<K extends keyof Handlers>(
  kind: K,
  cb: Handlers[K] extends Set<infer F> ? F : never,
): Promise<Unlisten> {
  if (isDesktop) {
    return listen(TAURI_EVENT[kind], (e) => (cb as (p: unknown) => void)(e.payload));
  }
  sse();
  (handlers[kind] as Set<unknown>).add(cb);
  return () => (handlers[kind] as Set<unknown>).delete(cb);
}
