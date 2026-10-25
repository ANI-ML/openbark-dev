import { Button, StatusDot } from "@openbark/ui";
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  ClipboardPaste,
  Copy,
  GitCompare,
  GripVertical,
  Plus,
  RotateCcw,
  Share2,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import type {
  ContentKind,
  LintWarning,
  Standard,
  Template,
  TemplateCan,
  TemplateField,
  TemplatePart,
  TemplateSection,
  TemplateView,
  TemplatesMode,
} from "../engine";
import { examplesForTitle, NOTE_EXAMPLES } from "../promptExamples";
import { COMMON_TYPES, TEMPLATE_ICONS, TemplateIcon } from "../templateIcons";
import { builtinText, sourceLabel, templateDescription, templateName, withoutContext } from "../i18n";
import { BUILTIN_TEXT } from "../builtinText";
import { ExampleSheet } from "../components/ExampleSheet";
import { AlwaysAppliedPanel } from "../components/AlwaysAppliedPanel";
import { ApproveDialog } from "../components/ApproveDialog";
import { CompareSheet } from "../components/CompareSheet";
import { ShareDialog } from "../components/ShareDialog";
import { SHOW_FOR, speciesPlural, speciesValue } from "../species";
import type { TemplateCondition } from "../engine";
import { PHONE, useMediaQuery } from "../useMediaQuery";
import { badges, editorNotice, keptNotice, originalNotice, versionLabel } from "../templateLabels";

/** English names of the interface languages, for "In French: ..." hints. */
const LANGUAGE_NAME: Record<string, string> = { fr: "French", es: "Spanish", de: "German", pt: "Portuguese" };

/** Why a phrase conflicts with the rules that always apply, in plain words. */
const LINT_REASON: Record<engine.LintReason, string> = {
  unsaid_findings: "Findings that weren't said",
  unsaid_numbers: "Numbers that weren't said",
  unsaid_doses: "Doses that weren't said",
  removes_not_discussed: "It would take away \"Not discussed\"",
  inventing: "Inventing",
  evidence: "Every line keeps its link to the recording",
};

/** Lint warnings under the field they concern (ADR-0024 section 4.4): they warn and never block. */
function LintList({ warnings }: { warnings: LintWarning[] }) {
  const t = useT();
  if (warnings.length === 0) return null;
  return (
    <div className="lint" role="status">
      {warnings.map((w, i) => (
        <p key={i} className="ob-notice small">
          {t("\"{text}\": {reason}. OpenBark won't do this. The rules that always apply win: nothing is written that wasn't said, and every draft is checked.", {
            text: w.text,
            reason: t(LINT_REASON[w.reason]),
          })}
        </p>
      ))}
    </div>
  );
}

/** "In French: Plan" with Use this, under unchanged built-in text (ADR-0024 section 12). */
function TranslationHint({ text, show, onUse }: { text: string; show: boolean; onUse: (translated: string) => void }) {
  const app = useApp();
  const t = useT();
  const lang = app.settings.uiLanguage;
  if (!show || lang === "en" || !text || !BUILTIN_TEXT.has(text)) return null;
  const translated = t(text);
  if (translated === text) return null;
  return (
    <span className="tb-hint small faint">
      {t("In {language}: {text}", { language: t(LANGUAGE_NAME[lang] ?? lang), text: translated })}{" "}
      <button type="button" className="link small" onClick={() => onUse(translated)}>
        {t("Use this")}
      </button>
    </span>
  );
}

/** "Show only for cats": which species a heading is drafted for (DESIGN.md §5 conditionals as sentences). */
function ShowFor({
  label,
  when,
  readOnly,
  onChange,
}: {
  label: string;
  when: TemplateCondition | null | undefined;
  readOnly: boolean;
  onChange: (when: TemplateCondition | null) => void;
}) {
  const t = useT();
  const current = speciesValue(when?.species);
  const known = SHOW_FOR.some((o) => speciesValue(o.species) === current);
  return (
    <label className="tb-when small">
      <span className="muted">{t("Show")}</span>
      <select
        className="ob-select"
        aria-label={t("Which species {label} is shown for", { label })}
        disabled={readOnly}
        value={known ? current : "custom"}
        onChange={(e) => {
          if (e.target.value === "custom") return;
          onChange(e.target.value ? { species: e.target.value.split(",") } : null);
        }}
      >
        {!known && (
          <option value="custom">
            {t("only for {species}", { species: (when?.species ?? []).map((s) => t(speciesPlural(s))).join(", ") })}
          </option>
        )}
        {SHOW_FOR.map((o) => (
          <option key={o.label} value={speciesValue(o.species)}>
            {t(o.label)}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Templates as grouped rows with their icons and badges (ADR-0024 section
 * 14.3): only you, the hospital's (or yours, working alone), common visits,
 * specialty, and client documents. Each badge is a dot with text.
 */
function TemplateGroups({
  list,
  mode,
  onOpen,
}: {
  list: engine.TemplateSummary[];
  mode: TemplatesMode;
  onOpen: (id: string) => void;
}) {
  const t = useT();
  const onlyYou = (x: engine.TemplateSummary) => x.layer === "personal" && !x.edited && !x.builtin;
  const groups: { title: string; items: engine.TemplateSummary[] }[] = [
    { title: "Only you", items: list.filter(onlyYou) },
    { title: mode === "alone" ? "Your templates" : "Hospital templates", items: list.filter((x) => !x.builtin && !onlyYou(x)) },
    {
      title: "Common visits",
      items: COMMON_TYPES.map((id) => list.find((x) => x.id === id && x.builtin)).filter((x): x is engine.TemplateSummary => !!x),
    },
    { title: "Specialty and other notes", items: list.filter((x) => x.builtin && x.document !== "client" && !COMMON_TYPES.includes(x.id)) },
    { title: "For the client", items: list.filter((x) => x.builtin && x.document === "client") },
  ];
  return (
    <>
      {groups
        .filter((g) => g.items.length > 0)
        .map((g) => (
          <section key={g.title} className="tpl-group" aria-label={t(g.title)}>
            <h2 className="tpl-group-title">{t(g.title)}</h2>
            <ul className="tpl-list">
              {g.items.map((tpl) => (
                <li key={tpl.id}>
                  <button type="button" className="tpl-row" onClick={() => onOpen(tpl.id)}>
                    <span className="type-icon">
                      <TemplateIcon template={tpl} size={20} />
                    </span>
                    <span className="tpl-text">
                      <span className="tpl-name">{templateName(t, tpl.name)}</span>
                      <span className="small muted tpl-desc">{templateDescription(t, tpl)}</span>
                      {badges(t, tpl, mode).length > 0 && (
                        <span className="tpl-badges">
                          {badges(t, tpl, mode).map((b) => (
                            <StatusDot key={b.text} tone={b.tone}>
                              {b.text}
                            </StatusDot>
                          ))}
                        </span>
                      )}
                    </span>
                    <span className="tpl-meta small muted">
                      {t("{n} headings", { n: tpl.headings })}
                      {tpl.standards > 0 && <span>{t("{n} standards", { n: tpl.standards })}</span>}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
    </>
  );
}

/** The icon for a template, chosen from a small set (built-ins name theirs). */
function IconPicker({
  value,
  document,
  readOnly,
  onChange,
}: {
  value: string | undefined;
  document: Template["document"];
  readOnly: boolean;
  onChange: (icon: string | undefined) => void;
}) {
  const t = useT();
  return (
    <div className="ob-field icon-field">
      <span className="label">{t("Icon")}</span>
      <div className="icon-picker" role="radiogroup" aria-label={t("Icon")}>
        {Object.entries(TEMPLATE_ICONS).map(([name, I]) => (
          <button
            key={name}
            type="button"
            role="radio"
            aria-checked={value === name || (!value && name === (document === "client" ? "chat-text" : "clipboard-text"))}
            aria-label={name.replace(/-/g, " ")}
            className="icon-choice"
            disabled={readOnly}
            onClick={() => onChange(name)}
          >
            <I size={18} aria-hidden />
          </button>
        ))}
      </div>
    </div>
  );
}

/** Append an example sentence to an instruction (or start one). */
function appendSentence(text: string | null | undefined, sentence: string): string {
  const base = (text ?? "").trim();
  if (!base) return sentence;
  return `${base}${/[.!?]$/.test(base) ? "" : "."} ${sentence}`;
}

/** Example instructions for one heading: its own, or ones matched on its title. */
function InstructionExamples({
  label,
  own,
  readOnly,
  builtin,
  onPick,
}: {
  label: string;
  own: string[] | undefined;
  readOnly?: boolean;
  /** A built-in's examples show in the app language. */
  builtin: boolean;
  onPick: (example: string) => void;
}) {
  const t = useT();
  const examples = own?.length ? own : examplesForTitle(label);
  if (examples.length === 0) return null;
  const shown = (e: string) => builtinText(t, e, builtin);
  return (
    <div className="tb-examples" aria-label={t("Example instructions for {label}", { label: shown(label) })}>
      <span className="small faint">{t("Examples")}</span>
      {examples.map((e) => (
        <button key={e} type="button" className="ob-chip ask-chip" disabled={readOnly} onClick={() => onPick(e)}>
          {shown(e)}
        </button>
      ))}
    </div>
  );
}

/** Who may create templates: working alone, a person who drafts notes, or an admin. */
function canCreate(mode: TemplatesMode, can: { notes: boolean; admin: boolean }): boolean {
  if (mode === "alone") return true;
  if (mode === "hospital") return can.admin;
  return can.notes || can.admin;
}

/**
 * DESIGN.md §5 Template builder, with per-person versions (ADR-0024): any
 * template can be changed, built-ins included. In a team the change is saved
 * as your version; the hospital's templates are everyone's defaults, and an
 * admin changes them with their PIN. Working alone, changes go to the
 * templates folder.
 */
export function Templates({ initialOpen, initialHeading }: { initialOpen?: string; initialHeading?: string }) {
  const app = useApp();
  const t = useT();
  const [open, setOpen] = useState<{ id: string | null; from?: Template; key?: number; heading?: string } | null>(
    initialOpen ? { id: initialOpen, heading: initialHeading } : null,
  );
  const [list, setList] = useState<engine.TemplateSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fromExample, setFromExample] = useState(false);
  const [setupNotice, setSetupNotice] = useState(false);
  const phone = useMediaQuery(PHONE);
  const mode: TemplatesMode = app.status?.templates ?? (app.status?.team ? "personal" : "alone");
  const who = app.user?.id ?? "";
  const mayCreate = canCreate(mode, app.can);

  // The list is this person's: it reloads when someone else starts using the app.
  const refresh = useCallback(() => engine.listTemplates().then(setList, (e) => setError(String(e?.message ?? e))), []);
  useEffect(() => {
    setList(null);
    void refresh();
  }, [refresh, who]);
  useEffect(() => {
    if (mode === "alone" || !app.can.admin) return;
    engine.teamSettings().then((s) => setSetupNotice(s.setup_notice), () => {});
  }, [mode, app.can.admin, who]);

  if (open) {
    return (
      <Builder
        // A new template or a copy remounts the builder, so no state carries over.
        key={`${open.key ?? ""}:${who}`}
        id={open.id}
        from={open.from}
        heading={open.heading}
        onClose={() => {
          setOpen(null);
          void refresh();
        }}
        onOpen={(id, from) => setOpen({ id, from, key: Date.now() })}
        onSaved={() => void refresh()}
      />
    );
  }

  const startFromSoap = async () => {
    try {
      const soap = await engine.template("soap");
      setOpen({ id: null, from: copyOf(soap.template, t("SOAP note copy")), key: Date.now() });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const secondary = (size?: "compact") => (
    <>
      <Button size={size} onClick={() => setFromExample(true)}>
        <ClipboardPaste size={size ? 16 : 20} aria-hidden />
        {t("From an example note")}
      </Button>
      <Button size={size} onClick={startFromSoap}>
        <Copy size={size ? 16 : 20} aria-hidden />
        {t("Start from SOAP")}
      </Button>
    </>
  );

  return (
    <div className="screen templates">
      {/* Phones (§9): the header is the title and the one primary action; the other two sit under the intro. */}
      <header className="screen-head">
        <div className="head-title">
          {/* Phones reach Templates from Settings (ADR-0027): the way back. */}
          <Button variant="ghost" size="compact" className="back-to-settings" onClick={() => app.navigate({ screen: "settings" })}>
            <ArrowLeft size={16} aria-hidden />
            {t("Settings")}
          </Button>
          <h1>{t("Templates")}</h1>
        </div>
        {mayCreate && (
          <div className="head-actions">
            {!phone && secondary()}
            <Button variant="primary" onClick={() => setOpen({ id: null, from: blank(), key: Date.now() })}>
              <Plus size={20} aria-hidden />
              {t("New template")}
            </Button>
          </div>
        )}
      </header>
      <p className="small muted measure">
        {mode === "alone"
          ? t("A template is the headings of a note and what goes under each. Change any template. Reset to original brings back the built-in at any time.")
          : mode === "hospital"
            ? t("A template is the headings of a note and what goes under each. Templates are shared by the hospital: everyone drafts with the same ones, and an admin changes them.")
            : t("A template is the headings of a note and what goes under each. Change any template: your changes are saved as your version, and everyone else keeps the original.")}{" "}
        {t("The model fills every heading from the recording and links each line to where it was said; anything not discussed is shown as not discussed.")}
      </p>
      {phone && mayCreate && <div className="head-actions">{secondary("compact")}</div>}
      {setupNotice && (
        <div className="ob-notice setup-notice" role="status">
          <span>{t("Templates you changed while working alone are now the hospital's. Everyone can make their own version.")}</span>
          <Button
            size="compact"
            variant="ghost"
            onClick={() => engine.dismissSetupNotice().then(() => setSetupNotice(false), () => setSetupNotice(false))}
          >
            {t("Got it")}
          </Button>
        </div>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(error)}
        </p>
      )}
      {list === null ? (
        <div className="ob-skeleton" style={{ height: 120 }} />
      ) : (
        <TemplateGroups list={list} mode={mode} onOpen={(id) => setOpen({ id, key: Date.now() })} />
      )}
      {fromExample && (
        <ExampleSheet
          onClose={() => setFromExample(false)}
          onProposed={(proposed) => {
            setFromExample(false);
            setOpen({ id: null, from: proposed, key: Date.now() });
          }}
        />
      )}
    </div>
  );
}

// ---- builder ------------------------------------------------------------------

type Kind = ContentKind | "parts";
const KINDS: { value: Kind; label: string }[] = [
  { value: "statements", label: "Bullet points" },
  { value: "text", label: "Text box" },
  { value: "fields", label: "Measurements" },
  { value: "medications", label: "Medications" },
  { value: "teeth", label: "Tooth chart" },
  { value: "parts", label: "Subheadings" },
];

let uidCounter = 0;
const uid = () => `u${++uidCounter}`;
type WithUid<T> = T & { _uid?: string };

function withUids(t: Template): Template {
  return {
    ...t,
    sections: t.sections.map((s) => ({
      ...s,
      _uid: uid(),
      ...(s.parts ? { parts: s.parts.map((p) => ({ ...p, _uid: uid() })) } : {}),
    })) as TemplateSection[],
  };
}

/** The template as the engine takes it: no UI ids, no standards or origin (kept separately). */
function clean(t: Template): Template {
  const json = JSON.stringify({ ...t, standards: undefined, origin: undefined }, (k, v) => (k === "_uid" ? undefined : v));
  return JSON.parse(json) as Template;
}

function blank(): Template {
  return { id: "", version: "1", name: "", description: "", sections: [{ id: "", title: "", kind: "statements" }] };
}

function copyOf(t: Template, name: string): Template {
  return { ...clean(t), id: "", version: "1", name };
}

function move<T>(items: T[], from: number, to: number): T[] {
  if (to < 0 || to >= items.length || from === to) return items;
  const next = items.slice();
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}

interface BuilderProps {
  id: string | null;
  from?: Template;
  /** Open scrolled to this heading ("plan/medications"), from Review's Ask. */
  heading?: string;
  onClose: () => void;
  onOpen: (id: string | null, from?: Template) => void;
  onSaved: () => void;
}

/** An action waiting for an admin's PIN. */
type Pending =
  | { kind: "save"; hospital: boolean }
  | { kind: "reset-team" }
  | { kind: "delete" }
  | { kind: "keep"; fingerprint: string }
  | { kind: "remove-standard"; standard: Standard };

const NO_EDIT: TemplateCan = {
  edit: false,
  save_to: null,
  save_needs_pin: false,
  save_for_hospital: false,
  reset: false,
  share: false,
  reset_team: false,
  delete: false,
  delete_needs_pin: false,
  keep: false,
  keep_needs_pin: false,
  duplicate: false,
  change_document: false,
  standards_personal: false,
  standards_team_needs_pin: false,
  admin: false,
};

function Builder({ id, from, heading, onClose, onOpen, onSaved }: BuilderProps) {
  const app = useApp();
  const t = useT();
  /** The template as stored for this person (null for a new one). */
  const [view, setView] = useState<TemplateView | null>(null);
  const [draft, setDraft] = useState<Template | null>(from ? withUids(from) : null);
  const [saved, setSaved] = useState<Template | null>(null);
  const [preview, setPreview] = useState<TemplateView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [tab, setTab] = useState<"note" | "prompt">("note");
  const [confirm, setConfirm] = useState<"delete" | "leave" | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [comparing, setComparing] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [fromExample, setFromExample] = useState(false);
  const [encounters, setEncounters] = useState<engine.SessionSummary[]>([]);
  const [tryOn, setTryOn] = useState("");
  const nameRef = useRef<HTMLInputElement>(null);

  const load = useCallback((v: TemplateView) => {
    setView(v);
    setDraft(withUids(v.template));
    setSaved(clean(v.template));
    setConflict(false);
  }, []);

  useEffect(() => {
    if (id) {
      engine.template(id).then(load, (e) => setError(String(e?.message ?? e)));
    } else {
      nameRef.current?.focus();
    }
    engine.listSessions().then(
      (rows) => {
        setEncounters(rows.slice(0, 20));
        setTryOn(rows[0]?.id ?? "");
      },
      () => {},
    );
  }, [id, load]);

  // Opened from Review's Ask: scroll to that heading's instruction.
  useEffect(() => {
    if (!heading || !draft) return;
    const el = document.getElementById(`tb-${heading.replace("/", "-")}`);
    el?.scrollIntoView({ block: "center" });
    el?.focus();
    // Only once the template is in.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [heading, view]);

  // Live check, lint and prompt preview, a moment after typing stops.
  useEffect(() => {
    if (!draft) return;
    const timer = setTimeout(() => {
      engine.previewTemplate(clean(draft)).then(
        (v) => {
          setPreview(v);
          setProblem(null);
        },
        (e) => setProblem(e instanceof Error ? e.message : String(e)),
      );
    }, 300);
    return () => clearTimeout(timer);
  }, [draft]);

  const dirty = useMemo(
    () => !saved || (draft != null && JSON.stringify(clean(draft)) !== JSON.stringify(saved)),
    [draft, saved],
  );
  const can: TemplateCan = view?.can ?? preview?.can ?? NO_EDIT;
  const isNew = !saved;
  const readOnly = !can.edit;
  const builtinId = view?.builtin ?? false;
  const mode: TemplatesMode = view?.mode ?? preview?.mode ?? app.status?.templates ?? "alone";

  if (!draft) {
    return (
      <div className="screen">
        {error ? (
          <p className="ob-notice" data-tone="danger" role="alert">
            {t(error)}
          </p>
        ) : (
          <div className="ob-skeleton" style={{ height: 240 }} />
        )}
      </div>
    );
  }

  const set = (patch: Partial<Template>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const setSections = (f: (s: TemplateSection[]) => TemplateSection[]) =>
    setDraft((d) => (d ? { ...d, sections: f(d.sections) } : d));
  const setSection = (i: number, patch: Partial<TemplateSection>) =>
    setSections((ss) => ss.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  /** Where Save puts a change in a team, when it's the hospital's templates. */
  const toHospital = (hospital: boolean) => hospital || (can.save_to === "team" && mode !== "alone");

  async function save(hospital = false, approver?: engine.Approver, rebase = false) {
    if (!draft) return;
    const needsPin = mode !== "alone" && (hospital || can.save_needs_pin);
    if (needsPin && !approver) {
      setPending({ kind: "save", hospital });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // Saving over a newer version (after a conflict) starts from what's stored now.
      const base = rebase && id ? await engine.template(id) : view;
      const v = await engine.saveTemplate({
        template: clean(draft),
        base_version: base?.template.version ?? null,
        base_layer: base?.layer ?? null,
        hospital: toHospital(hospital),
        approver: approver ?? null,
      });
      const wasNew = !saved;
      // Your version: personal in a team; working alone, the folder's copy of a built-in.
      const yours = !!v.original && (v.layer === "personal" || (v.mode === "alone" && v.layer === "team"));
      const firstVersion = yours && view?.layer !== v.layer;
      app.toast(
        yours
          ? firstVersion
            ? t("Saved as your version {n}", { n: v.template.version })
            : t("Your version saved, version {n}", { n: v.template.version })
          : toHospital(hospital)
            ? t("Saved for the hospital, version {v}", { v: v.template.version })
            : t("Template saved, version {v}", { v: v.template.version }),
      );
      setPending(null);
      onSaved();
      if (wasNew) onOpen(v.template.id);
      else load(v);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (approver && /PIN|admin/.test(message)) throw e; // shown in the approval dialog
      setPending(null);
      setConflict(/changed while you were editing/.test(message));
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  async function reset() {
    if (!id) return;
    try {
      const r = await engine.resetTemplate(id);
      load(r.view);
      onSaved();
      app.toast(t("Reset to the original"), {
        label: t("Undo"),
        run: () => void engine.undoResetTemplate(id, r.undo).then((v) => (load(v), onSaved()), fail),
      });
    } catch (e) {
      fail(e);
    }
  }

  async function resetTeam(approver: engine.Approver) {
    if (!id) return;
    const r = await engine.resetTeamTemplate(id, approver);
    setPending(null);
    load(r.view);
    onSaved();
    app.toast(t("The hospital's version reset to the built-in"), {
      label: t("Undo"),
      run: () => void engine.undoResetTemplate(id, r.undo).then((v) => (load(v), onSaved()), fail),
    });
  }

  async function keep(fingerprint: string, approver?: engine.Approver) {
    if (!id) return;
    if (can.keep_needs_pin && mode !== "alone" && !approver) {
      setPending({ kind: "keep", fingerprint });
      return;
    }
    const v = await engine.keepTemplate(id, fingerprint, approver);
    setPending(null);
    load(v);
    onSaved();
    app.toast(t("Kept your version"));
  }

  /** "Use the original": reset whichever version this person sees. */
  function useOriginal() {
    if (view?.layer === "team" && mode !== "alone") setPending({ kind: "reset-team" });
    else void reset();
  }

  async function remove(approver?: engine.Approver) {
    if (!saved) return;
    if (can.delete_needs_pin && mode !== "alone" && !approver) {
      setPending({ kind: "delete" });
      return;
    }
    try {
      await engine.deleteTemplate(saved.id, approver);
      setPending(null);
      app.toast(t("Template deleted"));
      onClose();
    } catch (e) {
      if (approver) throw e;
      fail(e);
    }
  }

  async function share(shareMode: "replace" | "add", name: string | null, approver: engine.Approver) {
    if (!id) return;
    const v = await engine.shareTemplate(id, shareMode, name, approver);
    setSharing(false);
    app.toast(t("Shared with the hospital"));
    onSaved();
    if (v.template.id !== id) onOpen(v.template.id);
    else load(v);
  }

  async function dropStandard(s: Standard, approver?: engine.Approver) {
    if (!saved) return;
    const team = s.scope !== "personal";
    if (team && mode !== "alone" && !approver) {
      setPending({ kind: "remove-standard", standard: s });
      return;
    }
    const v = await engine.removeStandard(saved.id, s.id, approver);
    setPending(null);
    setView(v);
    app.toast(t("Standard removed"));
  }

  async function mute(s: Standard, muted: boolean) {
    if (!saved) return;
    try {
      setView(await engine.muteStandard(saved.id, s.id, muted));
      app.toast(muted ? t("Turned off for you") : t("Turned back on for you"));
    } catch (e) {
      fail(e);
    }
  }

  function tryTemplate() {
    if (!saved || !tryOn) return;
    if (saved.id !== app.settings.template) app.setSettings({ template: saved.id });
    app.navigate({ screen: "review", id: tryOn, template: saved.id });
  }

  /** Every unchanged built-in heading, label and measurement in the app language (keys don't change). */
  function useTranslatedHeadings() {
    const tr = (x: string) => (BUILTIN_TEXT.has(x) ? t(x) : x);
    setSections((ss) =>
      ss.map((s) => ({
        ...s,
        title: tr(s.title),
        fields: s.fields?.map((f) => ({ ...f, label: tr(f.label) })),
        parts: s.parts?.map((p) => ({ ...p, label: tr(p.label), fields: p.fields?.map((f) => ({ ...f, label: tr(f.label) })) })),
      })),
    );
  }

  const standards = view?.template.standards ?? [];
  const muted = new Set(view?.muted ?? []);
  const standardsFor = (path: string) => standards.filter((s) => s.heading === path);
  const wholeNote = standardsFor("");
  const lint = preview?.lint ?? [];
  const lintFor = (path: string) => lint.filter((w) => w.path === path);
  /** Lint paths name headings by key; new headings get their key from the preview, by position. */
  const headingPath = (i: number, j?: number) => {
    const s = preview?.template.sections[i];
    if (!s) return "";
    return j == null ? `headings/${s.id}` : `headings/${s.id}/${s.parts?.[j]?.key ?? ""}`;
  };
  const translateHints = !readOnly && builtinId && app.settings.uiLanguage !== "en";
  const notice = editorNotice(t, view, isNew);
  const changed = view ? originalNotice(t, view) : null;
  const kept = view ? keptNotice(t, view) : null;
  const canStandardRemove = (s: Standard) =>
    s.scope === "personal" ? can.standards_personal : mode === "alone" || can.admin || (can.edit && can.standards_team_needs_pin);

  // The one primary button (§5): Save my version, Save, or Save for the hospital.
  const primaryLabel =
    can.save_to === "team" && mode !== "alone"
      ? t("Save for the hospital")
      : view && ((can.save_to === "personal" && view.layer !== "personal") || (mode === "alone" && view.layer === "builtin"))
        ? t("Save my version")
        : t("Save");

  return (
    <div className="screen builder">
      <header className="screen-head">
        <div className="builder-title">
          {confirm === "leave" ? (
            <span className="inline-confirm">
              <span className="small">{t("Discard unsaved changes?")}</span>
              <Button onClick={onClose}>{t("Discard changes")}</Button>
              <Button variant="ghost" onClick={() => setConfirm(null)}>
                {t("Keep editing")}
              </Button>
            </span>
          ) : (
            <Button variant="ghost" onClick={() => (dirty && !readOnly ? setConfirm("leave") : onClose())}>
              <ArrowLeft size={20} aria-hidden />
              {t("Templates")}
            </Button>
          )}
          <h1>{templateName(t, draft.name) || t("New template")}</h1>
          {view && <span className="mono faint">{versionLabel(t, view)}</span>}
        </div>
        <div className="head-actions">
          {view && view.original && view.layer !== "builtin" && (
            <Button variant="ghost" onClick={() => setComparing(true)}>
              <GitCompare size={20} aria-hidden />
              {t("Compare with original")}
            </Button>
          )}
          {can.reset && view?.original && (
            <Button variant="ghost" onClick={() => void reset()}>
              <RotateCcw size={20} aria-hidden />
              {t("Reset to original")}
            </Button>
          )}
          {can.reset_team && (
            <Button variant="ghost" onClick={() => setPending({ kind: "reset-team" })}>
              <RotateCcw size={20} aria-hidden />
              {t("Reset the hospital's version")}
            </Button>
          )}
          {can.share && saved && (
            <Button variant="ghost" onClick={() => setSharing(true)}>
              <Share2 size={20} aria-hidden />
              {t("Share with the hospital")}
            </Button>
          )}
          {saved && can.duplicate && (
            <Button onClick={() => onOpen(null, copyOf(draft, t("{name} copy", { name: templateName(t, draft.name) })))}>
              <Copy size={20} aria-hidden />
              {t("Duplicate")}
            </Button>
          )}
          {saved &&
            can.delete &&
            (confirm === "delete" ? (
              <span className="inline-confirm">
                <Button variant="danger" onClick={() => void remove()}>
                  {t("Delete for good")}
                </Button>
                <Button variant="ghost" onClick={() => setConfirm(null)}>
                  {t("Keep")}
                </Button>
              </span>
            ) : (
              <Button variant="ghost" onClick={() => setConfirm("delete")}>
                <Trash2 size={20} aria-hidden />
                {t("Delete")}
              </Button>
            ))}
          {can.save_for_hospital && (
            <Button onClick={() => void save(true)} disabled={busy || problem != null}>
              {t("Save for the hospital")}
            </Button>
          )}
          {!readOnly && (
            <Button variant="primary" onClick={() => void save()} disabled={busy || !dirty || problem != null}>
              {primaryLabel}
            </Button>
          )}
        </div>
      </header>

      {notice && <p className="ob-notice builder-notice">{notice}</p>}
      {changed && view && (
        <div className="ob-notice original-notice" role="status">
          <span>{changed}</span>
          <span className="notice-actions">
            {view.has_base || view.original ? (
              <Button size="compact" variant="ghost" onClick={() => setComparing(true)}>
                {t("Compare")}
              </Button>
            ) : null}
            {can.keep && view.original_changed?.fingerprint && (
              <Button size="compact" variant="ghost" onClick={() => void keep(view.original_changed?.fingerprint ?? "").catch(fail)}>
                {t("Keep mine")}
              </Button>
            )}
            {view.original && (can.reset || can.reset_team || mode === "alone") && (
              <Button size="compact" variant="ghost" onClick={useOriginal}>
                {t("Use the original")}
              </Button>
            )}
          </span>
        </div>
      )}
      {kept && <p className="small muted">{kept}</p>}
      {error && (
        <div className="ob-notice" data-tone="danger" role="alert">
          <span>{t(error)}</span>
          {conflict && (
            <span className="notice-actions">
              <Button size="compact" onClick={() => void save(false, undefined, true)}>
                {t("Save over it")}
              </Button>
              <Button size="compact" variant="ghost" onClick={() => id && engine.template(id).then(load, fail)}>
                {t("Load the saved version")}
              </Button>
            </span>
          )}
        </div>
      )}

      <div className="builder-meta">
        <label className="ob-field">
          <span className="label">{t("Name")}</span>
          <input
            ref={nameRef}
            className="ob-input"
            value={readOnly ? templateName(t, draft.name) : draft.name}
            disabled={readOnly}
            placeholder={t("Dental procedure")}
            onChange={(e) => set({ name: e.target.value })}
          />
        </label>
        <label className="ob-field">
          <span className="label">{t("Description")}</span>
          <input
            className="ob-input"
            value={readOnly ? templateDescription(t, { description: draft.description, builtin: builtinId }) : (draft.description ?? "")}
            disabled={readOnly}
            placeholder={t("Who uses it, and when")}
            onChange={(e) => set({ description: e.target.value })}
          />
        </label>
        <IconPicker value={draft.icon} document={draft.document} readOnly={readOnly} onChange={(icon) => set({ icon })} />
        <label className="ob-field">
          <span className="label">{t("Level of detail")}</span>
          <select
            className="ob-select"
            value={draft.detail ?? "standard"}
            disabled={readOnly}
            onChange={(e) => set({ detail: e.target.value === "standard" ? undefined : (e.target.value as Template["detail"]) })}
          >
            <option value="concise">{t("Concise: the fewest words that keep every fact")}</option>
            <option value="standard">{withoutContext(t("Standard (level of detail)"))}</option>
            <option value="thorough">{t("Thorough: every relevant detail that was said")}</option>
          </select>
        </label>
        <label className="ob-field">
          <span className="label">{t("Who it's for")}</span>
          <select
            className="ob-select"
            value={draft.document ?? "record"}
            disabled={readOnly || (!isNew && !can.change_document)}
            onChange={(e) => set({ document: e.target.value === "client" ? "client" : undefined })}
          >
            <option value="record">{t("The medical record (signed by a veterinarian)")}</option>
            <option value="client">{t("The client (follows the signed note; never signed)")}</option>
          </select>
          {!isNew && !can.change_document && (
            <span className="ob-help">{t("Fixed for this visit type. Duplicate the template to make one for the client.")}</span>
          )}
        </label>
        <details className="builder-advanced">
          <summary className="label">{t("Instructions and rules for the whole note")}</summary>
          <label className="ob-field">
            <span className="label">{t("Instructions")}</span>
            <textarea
              className="ob-input tb-text"
              rows={4}
              disabled={readOnly}
              value={draft.instructions ?? ""}
              placeholder={t("Write the medical record for this consultation under the headings below. Leave empty for the default.")}
              onChange={(e) => set({ instructions: e.target.value || undefined })}
            />
          </label>
          <LintList warnings={lintFor("instructions")} />
          <label className="ob-field">
            <span className="label">{t("Extra rules, one per line")}</span>
            <textarea
              className="ob-input tb-text"
              rows={3}
              disabled={readOnly}
              value={(draft.rules ?? []).join("\n")}
              placeholder={t("Record the anaesthetic monitoring interval when stated.")}
              onChange={(e) => set({ rules: e.target.value.split("\n") })}
            />
          </label>
          <LintList warnings={lint.filter((w) => w.path.startsWith("rules/"))} />
          {!readOnly && (
            <div className="tb-examples" aria-label={t("Examples by kind of practice")}>
              <span className="small faint">{t("Examples by kind of practice")}</span>
              {NOTE_EXAMPLES.map((x) => (
                <button
                  key={x.useCase}
                  type="button"
                  className="ob-chip ask-chip"
                  title={x.text}
                  onClick={() => set({ rules: [...(draft.rules ?? []).filter((r) => r.trim()), x.text] })}
                >
                  {t(x.useCase)}
                </button>
              ))}
            </div>
          )}
          {(preview ?? view) && <AlwaysAppliedPanel rules={(preview ?? view)!.always_applied} />}
          <p className="ob-help">
            {t("Built-in templates are tested on scripted consults. Your changes aren't, but every draft is still checked against the recording.")}
          </p>
          {wholeNote.length > 0 && (
            <StandardList
              title={t("Standards for the whole note")}
              items={wholeNote}
              muted={muted}
              canMute={can.standards_personal && mode === "personal"}
              canRemove={canStandardRemove}
              onRemove={(s) => void dropStandard(s).catch(fail)}
              onMute={mute}
            />
          )}
        </details>
      </div>

      {(view?.not_used ?? []).length > 0 && (
        <StandardList
          title={t("Not used by your version")}
          items={view?.not_used ?? []}
          muted={muted}
          canMute={false}
          canRemove={canStandardRemove}
          onRemove={(s) => void dropStandard(s).catch(fail)}
          onMute={mute}
        />
      )}

      <div className="builder-split">
        <section className="pane builder-structure" aria-label={t("Headings")}>
          {(!readOnly || translateHints) && (
            <div className="tb-structure-actions">
              {!readOnly && (
                <Button variant="ghost" size="compact" onClick={() => setFromExample(true)}>
                  <ClipboardPaste size={16} aria-hidden />
                  {t("Update from an example note")}
                </Button>
              )}
              {translateHints && LANGUAGE_NAME[app.settings.uiLanguage] && (
                <Button variant="ghost" size="compact" onClick={useTranslatedHeadings}>
                  {t("Use {language} headings", { language: t(LANGUAGE_NAME[app.settings.uiLanguage] ?? "") })}
                </Button>
              )}
            </div>
          )}
          <ol className="tb-list">
            {draft.sections.map((s, i) => (
              <HeadingRow
                key={(s as WithUid<TemplateSection>)._uid ?? i}
                section={s}
                index={i}
                count={draft.sections.length}
                readOnly={readOnly}
                builtin={builtinId}
                hints={translateHints}
                lint={(j) => lintFor(headingPath(i, j))}
                standardsFor={standardsFor}
                standardProps={{ muted, canMute: can.standards_personal && mode === "personal", canRemove: canStandardRemove, onMute: mute }}
                onChange={(patch) => setSection(i, patch)}
                onMove={(to) => setSections((ss) => move(ss, i, to))}
                onDropFrom={(from) => setSections((ss) => move(ss, from, i))}
                onRemove={() => setSections((ss) => ss.filter((_, j) => j !== i))}
                onRemoveStandard={(st) => void dropStandard(st).catch(fail)}
              />
            ))}
          </ol>
          {!readOnly && (
            <Button
              onClick={() =>
                setSections((ss) => [...ss, { id: "", title: "", kind: "statements", _uid: uid() } as TemplateSection])
              }
            >
              <Plus size={20} aria-hidden />
              {t("Add heading")}
            </Button>
          )}
        </section>

        <section className="pane builder-preview" aria-label={t("Preview")}>
          <div className="ob-segmented" role="tablist">
            <button className="ob-segment" role="tab" aria-selected={tab === "note"} onClick={() => setTab("note")}>
              {t("Note")}
            </button>
            <button className="ob-segment" role="tab" aria-selected={tab === "prompt"} onClick={() => setTab("prompt")}>
              {t("What the model is told")}
            </button>
          </div>
          {problem && (
            <p className="ob-notice" role="status">
              {t(problem)}
            </p>
          )}
          {tab === "note" ? (
            <TemplatePreview template={preview?.template ?? draft} builtin={builtinId} />
          ) : (
            <>
              {app.settings.uiLanguage !== "en" && (
                <p className="small muted">{t("The model is instructed in English; notes are written in your chosen language.")}</p>
              )}
              <PromptText prompt={preview?.prompt ?? ""} locked={preview?.always_applied} />
            </>
          )}
          <div className="try-on">
            <span className="label">{t("Try on a visit")}</span>
            <div className="try-row">
              <select
                className="ob-select"
                aria-label={t("Visit to draft")}
                value={tryOn}
                onChange={(e) => setTryOn(e.target.value)}
                disabled={encounters.length === 0}
              >
                {encounters.length === 0 && <option value="">{t("No visits yet")}</option>}
                {encounters.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.created_at} {r.patient || sourceLabel(t, r.source)}
                  </option>
                ))}
              </select>
              <Button onClick={tryTemplate} disabled={!saved || dirty || !tryOn || !app.can.notes}>
                {t("Draft with this template")}
              </Button>
            </div>
            {(dirty || !saved) && !readOnly && <span className="ob-help">{t("Save the template first.")}</span>}
          </div>
        </section>
      </div>

      {comparing && view && id && (
        <CompareSheet
          templateId={id}
          hasBase={view.has_base ?? false}
          originalChanged={!!view.original_changed}
          canKeep={can.keep && !!view.original_changed?.fingerprint}
          onKeep={() => void keep(view.original_changed?.fingerprint ?? "").catch(fail)}
          onUseOriginal={useOriginal}
          onClose={() => setComparing(false)}
        />
      )}
      {sharing && view && (
        <ShareDialog
          name={templateName(t, draft.name)}
          hasOriginal={!!view.original}
          onShare={share}
          onCancel={() => setSharing(false)}
        />
      )}
      {fromExample && (
        <ExampleSheet
          onClose={() => setFromExample(false)}
          onProposed={(proposed) => {
            setFromExample(false);
            // The proposed headings replace the draft's; name, icon, level of detail and rules stay.
            setDraft((d) => (d ? { ...d, sections: withUids(proposed).sections } : d));
          }}
        />
      )}
      {pending && (
        <ApproveDialog
          title={
            pending.kind === "reset-team"
              ? t("Reset the hospital's version")
              : pending.kind === "delete"
                ? t("Delete a hospital template")
                : pending.kind === "remove-standard"
                  ? t("Remove a standard for everyone")
                  : pending.kind === "keep"
                    ? t("Keep the hospital's version")
                    : t("Save for the hospital")
          }
          body={t("Changes to the hospital's templates apply to everyone, so an admin approves each one with their PIN.")}
          action={
            pending.kind === "reset-team"
              ? t("Reset the hospital's version")
              : pending.kind === "delete"
                ? t("Delete for good")
                : pending.kind === "remove-standard"
                  ? t("Remove standard")
                  : pending.kind === "keep"
                    ? t("Keep mine")
                    : t("Save for the hospital")
          }
          onCancel={() => setPending(null)}
          onApprove={async (approver) => {
            if (pending.kind === "save") await save(pending.hospital, approver);
            else if (pending.kind === "reset-team") await resetTeam(approver);
            else if (pending.kind === "delete") await remove(approver);
            else if (pending.kind === "keep") await keep(pending.fingerprint, approver);
            else await dropStandard(pending.standard, approver);
          }}
        />
      )}
    </div>
  );
}

/** What the model is told, with the lines that always apply marked (they can't be changed). */
function PromptText({ prompt, locked }: { prompt: string; locked?: engine.AlwaysApplied }) {
  const t = useT();
  const fixed = new Set([...(locked?.core ?? []).map((r) => `- ${r.text}`), ...(locked?.fixed ?? []).map((r) => `- ${r}`)]);
  return (
    <pre className="prompt">
      {prompt.split("\n").map((line, i) =>
        fixed.has(line) ? (
          <span key={i} className="prompt-locked" title={t("Always applied")}>
            {line}
            {"\n"}
          </span>
        ) : (
          <span key={i}>
            {line}
            {"\n"}
          </span>
        ),
      )}
    </pre>
  );
}

interface StandardControls {
  muted: Set<string>;
  canMute: boolean;
  canRemove: (s: Standard) => boolean;
  onMute: (s: Standard, muted: boolean) => void;
}

interface HeadingRowProps {
  section: TemplateSection;
  index: number;
  count: number;
  readOnly: boolean;
  builtin: boolean;
  /** Offer "In French: ..." under unchanged built-in text. */
  hints: boolean;
  /** Lint warnings for the heading, or for subheading `j`. */
  lint: (j?: number) => LintWarning[];
  standardsFor: (heading: string) => Standard[];
  standardProps: StandardControls;
  onChange: (patch: Partial<TemplateSection>) => void;
  onMove: (to: number) => void;
  /** Another heading was dragged onto this one. */
  onDropFrom: (from: number) => void;
  onRemove: () => void;
  onRemoveStandard: (s: Standard) => void;
}

/** Drag state shared by rows of one list (HTML drag and drop, handle only). */
let dragging: { list: string; index: number } | null = null;

function HeadingRow({
  section: s,
  index,
  count,
  readOnly,
  builtin,
  hints,
  lint,
  standardsFor,
  standardProps,
  onChange,
  onMove,
  onDropFrom,
  onRemove,
  onRemoveStandard,
}: HeadingRowProps) {
  const t = useT();
  const kind: Kind = s.kind ?? "parts";
  const [armed, setArmed] = useState(false);
  const parts = s.parts ?? [];
  const setParts = (f: (p: TemplatePart[]) => TemplatePart[]) => onChange({ parts: f(parts) });
  const changeKind = (k: Kind) => {
    if (k === "parts") {
      onChange({
        kind: null,
        fields: [],
        parts: parts.length ? parts : [{ key: "", label: "", kind: "statements", _uid: uid() } as TemplatePart],
      });
    } else {
      onChange({
        kind: k,
        parts: [],
        fields: k === "fields" && !(s.fields ?? []).length ? [{ key: "", label: "" }] : s.fields,
        // A tooth chart is for dogs and cats (ADR-0021): never another animal's.
        ...(k === "teeth" ? { when: { species: ["canine", "feline"] } } : {}),
      });
    }
  };
  // Read-only, a built-in's text shows in the app language; editable, inputs show what notes will show.
  const shown = (x: string) => builtinText(t, x, builtin && readOnly);
  const label = builtinText(t, s.title, builtin) || t("Heading {n}", { n: index + 1 });

  return (
    <li
      className="tb-heading"
      draggable={armed && !readOnly}
      onDragStart={(e) => {
        dragging = { list: "sections", index };
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragEnd={() => {
        dragging = null;
        setArmed(false);
      }}
      onDragOver={(e) => dragging?.list === "sections" && e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        if (dragging?.list === "sections" && dragging.index !== index) onDropFrom(dragging.index);
        dragging = null;
      }}
    >
      <div className="tb-row">
        <span
          className="tb-handle"
          aria-hidden
          onPointerDown={() => setArmed(true)}
          onPointerUp={() => setArmed(false)}
          title={t("Drag to reorder")}
        >
          <GripVertical size={20} />
        </span>
        <input
          className="ob-input tb-title"
          aria-label={t("Heading {n} title", { n: index + 1 })}
          value={shown(s.title)}
          disabled={readOnly}
          placeholder={t("Heading, for example Anaesthesia")}
          onChange={(e) => onChange({ title: e.target.value })}
        />
        <select
          className="ob-select"
          aria-label={t("What goes under {label}", { label })}
          value={kind}
          disabled={readOnly}
          onChange={(e) => changeKind(e.target.value as Kind)}
        >
          {KINDS.map((k) => (
            <option key={k.value} value={k.value}>
              {t(k.label)}
            </option>
          ))}
        </select>
        {!readOnly && <RowTools label={label} index={index} count={count} onMove={onMove} onRemove={onRemove} />}
      </div>
      <TranslationHint text={s.title} show={hints} onUse={(title) => onChange({ title })} />
      <textarea
        id={`tb-${s.id}`}
        className="ob-input tb-text"
        rows={2}
        aria-label={t("Instruction for {label}", { label })}
        disabled={readOnly}
        value={s.instruction ?? ""}
        placeholder={t("Instruction for the model (optional): what belongs here and how to write it")}
        onChange={(e) => onChange({ instruction: e.target.value || null })}
      />
      <LintList warnings={lint()} />
      <InstructionExamples
        readOnly={readOnly}
        builtin={builtin}
        label={s.title}
        own={s.examples}
        onPick={(e) => onChange({ instruction: appendSentence(s.instruction, e) })}
      />
      <ShowFor label={label} when={s.when} readOnly={readOnly} onChange={(when) => onChange({ when })} />
      {kind === "fields" && (
        <FieldsEditor
          fields={s.fields ?? []}
          readOnly={readOnly}
          builtin={builtin}
          hints={hints}
          owner={label}
          onChange={(fields) => onChange({ fields })}
        />
      )}
      <StandardList title={t("Standards")} items={standardsFor(s.id)} onRemove={onRemoveStandard} {...standardProps} />
      {kind === "parts" && (
        <div className="tb-parts">
          <ol className="tb-list">
            {parts.map((p, j) => (
              <PartRow
                key={(p as WithUid<TemplatePart>)._uid ?? j}
                part={p}
                sectionId={s.id}
                index={j}
                count={parts.length}
                readOnly={readOnly}
                builtin={builtin}
                hints={hints}
                lint={lint(j)}
                standards={standardsFor(`${s.id}/${p.key}`)}
                standardProps={standardProps}
                onChange={(patch) => setParts((ps) => ps.map((x, k) => (k === j ? { ...x, ...patch } : x)))}
                onMove={(to) => setParts((ps) => move(ps, j, to))}
                onRemove={() => setParts((ps) => ps.filter((_, k) => k !== j))}
                onRemoveStandard={onRemoveStandard}
              />
            ))}
          </ol>
          {!readOnly && (
            <Button
              variant="ghost"
              onClick={() => setParts((ps) => [...ps, { key: "", label: "", kind: "statements", _uid: uid() } as TemplatePart])}
            >
              <Plus size={16} aria-hidden />
              {t("Add subheading")}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

function RowTools({
  label,
  index,
  count,
  onMove,
  onRemove,
}: {
  label: string;
  index: number;
  count: number;
  onMove: (to: number) => void;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <span className="tb-tools">
      <button type="button" className="icon-btn" aria-label={t("Move {label} up", { label })} disabled={index === 0} onClick={() => onMove(index - 1)}>
        <ChevronUp size={16} aria-hidden />
      </button>
      <button
        type="button"
        className="icon-btn"
        aria-label={t("Move {label} down", { label })}
        disabled={index === count - 1}
        onClick={() => onMove(index + 1)}
      >
        <ChevronDown size={16} aria-hidden />
      </button>
      <button type="button" className="icon-btn" aria-label={t("Remove {label}", { label })} onClick={onRemove}>
        <Trash2 size={16} aria-hidden />
      </button>
    </span>
  );
}

interface PartRowProps {
  part: TemplatePart;
  sectionId: string;
  index: number;
  count: number;
  readOnly: boolean;
  builtin: boolean;
  hints: boolean;
  lint: LintWarning[];
  standards: Standard[];
  standardProps: StandardControls;
  onChange: (patch: Partial<TemplatePart>) => void;
  onMove: (to: number) => void;
  onRemove: () => void;
  onRemoveStandard: (s: Standard) => void;
}

function PartRow({
  part: p,
  sectionId,
  index,
  count,
  readOnly,
  builtin,
  hints,
  lint,
  standards,
  standardProps,
  onChange,
  onMove,
  onRemove,
  onRemoveStandard,
}: PartRowProps) {
  const t = useT();
  const label = builtinText(t, p.label, builtin) || t("Subheading {n}", { n: index + 1 });
  return (
    <li className="tb-part">
      <div className="tb-row">
        <input
          className="ob-input tb-title"
          aria-label={t("Subheading {n} title", { n: index + 1 })}
          value={builtinText(t, p.label, builtin && readOnly)}
          disabled={readOnly}
          placeholder={t("Subheading")}
          onChange={(e) => onChange({ label: e.target.value })}
        />
        <select
          className="ob-select"
          aria-label={t("What goes under {label}", { label })}
          value={p.kind}
          disabled={readOnly}
          onChange={(e) => {
            const kind = e.target.value as ContentKind;
            onChange({
              kind,
              fields: kind === "fields" && !(p.fields ?? []).length ? [{ key: "", label: "" }] : p.fields,
              ...(kind === "teeth" ? { when: { species: ["canine", "feline"] } } : {}),
            });
          }}
        >
          {KINDS.filter((k) => k.value !== "parts").map((k) => (
            <option key={k.value} value={k.value}>
              {t(k.label)}
            </option>
          ))}
        </select>
        {!readOnly && <RowTools label={label} index={index} count={count} onMove={onMove} onRemove={onRemove} />}
      </div>
      <TranslationHint text={p.label} show={hints} onUse={(l) => onChange({ label: l })} />
      <textarea
        id={`tb-${sectionId}-${p.key}`}
        className="ob-input tb-text"
        rows={1}
        aria-label={t("Instruction for {label}", { label })}
        disabled={readOnly}
        value={p.instruction ?? ""}
        placeholder={t("Instruction (optional)")}
        onChange={(e) => onChange({ instruction: e.target.value || null })}
      />
      <LintList warnings={lint} />
      <InstructionExamples
        readOnly={readOnly}
        builtin={builtin}
        label={p.label}
        own={p.examples}
        onPick={(e) => onChange({ instruction: appendSentence(p.instruction, e) })}
      />
      <ShowFor label={label} when={p.when} readOnly={readOnly} onChange={(when) => onChange({ when })} />
      {p.kind === "fields" && (
        <FieldsEditor
          fields={p.fields ?? []}
          readOnly={readOnly}
          builtin={builtin}
          hints={hints}
          owner={label}
          onChange={(fields) => onChange({ fields })}
        />
      )}
      <StandardList title={t("Standards")} items={standards} onRemove={onRemoveStandard} {...standardProps} />
    </li>
  );
}

function FieldsEditor({
  fields,
  readOnly,
  builtin,
  hints,
  owner,
  onChange,
}: {
  fields: TemplateField[];
  readOnly: boolean;
  builtin: boolean;
  hints: boolean;
  owner: string;
  onChange: (f: TemplateField[]) => void;
}) {
  const t = useT();
  return (
    <div className="tb-fields">
      <span className="label muted">{t("Measurements")}</span>
      <ul>
        {fields.map((f, i) => (
          <li key={i}>
            <input
              className="ob-input"
              aria-label={t("{owner} measurement {n}", { owner, n: i + 1 })}
              value={builtinText(t, f.label, builtin && readOnly)}
              disabled={readOnly}
              placeholder={t("Weight")}
              title={hints && BUILTIN_TEXT.has(f.label) ? t(f.label) : undefined}
              onChange={(e) => onChange(fields.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))}
            />
            {!readOnly && (
              <button
                type="button"
                className="icon-btn"
                aria-label={t("Remove {label}", { label: f.label || t("measurement") })}
                onClick={() => onChange(fields.filter((_, j) => j !== i))}
              >
                <X size={16} aria-hidden />
              </button>
            )}
          </li>
        ))}
      </ul>
      {!readOnly && (
        <Button variant="ghost" onClick={() => onChange([...fields, { key: "", label: "" }])}>
          <Plus size={16} aria-hidden />
          {t("Add measurement")}
        </Button>
      )}
    </div>
  );
}

/** Standards under a heading: the hospital's and your own, each saying whose. */
function StandardList({
  title,
  items,
  muted,
  canMute,
  canRemove,
  onRemove,
  onMute,
}: {
  title: string;
  items: Standard[];
  onRemove: (s: Standard) => void;
} & StandardControls) {
  const t = useT();
  if (items.length === 0) return null;
  return (
    <div className="tb-standards">
      <span className="label muted">{title}</span>
      <ul>
        {items.map((s) => {
          const off = muted.has(s.id);
          return (
            <li key={s.id} data-muted={off || undefined}>
              <span className="ob-dot" aria-hidden />
              <span className="small">{s.text}</span>
              <span className="small faint">
                {s.scope === "personal" ? t("Yours") : t("Everyone")}
                {s.added_by ? `, ${s.added_by}` : ""}, {engine.fmtStamp(s.added_at)}
              </span>
              {canMute && s.scope !== "personal" && (
                <button type="button" className="link small" onClick={() => onMute(s, !off)}>
                  {off ? t("Turn on for me") : t("Turn off for me")}
                </button>
              )}
              {canRemove(s) && (
                <button
                  type="button"
                  className="icon-btn"
                  aria-label={t("Remove standard: {text}", { text: s.text })}
                  onClick={() => onRemove(s)}
                >
                  <X size={16} aria-hidden />
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

const KIND_HINT: Record<ContentKind, string> = {
  text: "Sentences that read as a paragraph, each linked to where it was said",
  statements: "Bullet points, each linked to where it was said",
  fields: "",
  medications: "Drug, dose, route, frequency, duration",
  teeth: "Teeth by Triadan number, for dogs and cats. Filled from what was said; teeth not mentioned stay blank.",
};

/** The note's shape: every heading, with what the model will put there. */
function TemplatePreview({ template, builtin }: { template: Template; builtin: boolean }) {
  const t = useT();
  // A built-in template's unchanged headings and measurements read in the app language.
  const text = (x: string) => builtinText(t, x, builtin);
  const body = (kind: ContentKind, fields: TemplateField[] | undefined, instruction?: string | null) => (
    <>
      {kind === "fields" ? (
        <dl className="fields">
          {(fields ?? []).map((f, i) => (
            <div key={i}>
              <dt className="label muted">{text(f.label) || t("Unnamed")}</dt>
              <dd className="not-discussed">{t("Not discussed")}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="not-discussed">{t(KIND_HINT[kind])}</p>
      )}
      {instruction && <p className="small faint preview-instruction">{instruction}</p>}
    </>
  );
  return (
    <article className="note preview-note" aria-label={t("Template preview")}>
      <h2 className="preview-name">{(builtin ? templateName(t, template.name) : template.name) || t("New template")}</h2>
      {template.sections.map((s, i) => (
        <section key={i} className="note-section">
          <header className="note-section-head">
            <h2>{text(s.title) || t("Untitled heading")}</h2>
          </header>
          {s.when?.species?.length ? (
            <p className="small muted preview-when">
              {t("Only for {species}", { species: s.when.species.map((x) => t(speciesPlural(x))).join(", ") })}
            </p>
          ) : null}
          {s.kind
            ? body(s.kind, s.fields, s.instruction)
            : (
                <>
                  {s.instruction && <p className="small faint preview-instruction">{s.instruction}</p>}
                  {(s.parts ?? []).map((p, j) => (
                    <div className="note-part" key={j}>
                      <h3 className="label">{text(p.label) || t("Untitled subheading")}</h3>
                      {p.when?.species?.length ? (
                        <p className="small muted preview-when">
                          {t("Only for {species}", { species: p.when.species.map((x) => t(speciesPlural(x))).join(", ") })}
                        </p>
                      ) : null}
                      {body(p.kind, p.fields, p.instruction)}
                    </div>
                  ))}
                </>
              )}
        </section>
      ))}
    </article>
  );
}
