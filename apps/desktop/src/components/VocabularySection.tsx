import { Button } from "@openbark/ui";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { ApproveDialog } from "./ApproveDialog";

/** Kinds of term; rendered through t() (scripts/i18n-keys.py INDIRECT). */
export const KIND_LABEL: Record<engine.VocabularyKind, string> = {
  drug: "Drug",
  vaccine: "Vaccine",
  product: "Product",
  condition: "Condition",
  procedure: "Procedure",
};

/** A change waiting for an admin's PIN (a team). */
type Pending = { kind: "save"; term: engine.ClinicTerm } | { kind: "remove"; term: string };

/** "apple, a pickle" as "heard as" words. */
export function heardWords(text: string): string[] {
  return text
    .split(/[,;]/)
    .map((w) => w.trim().replace(/\s+/g, " "))
    .filter(Boolean);
}

/**
 * Settings, Words and drug names (ADR-0031): the clinic's own terms, and how
 * speech recognition tends to write them. They're shown as hints on the
 * transcript, never changed in it, from the next recording. The list is the
 * hospital's: in a team, an admin approves each change with their PIN.
 */
export function VocabularySection() {
  const app = useApp();
  const t = useT();
  const [view, setView] = useState<engine.VocabularyView | null>(null);
  const [term, setTerm] = useState("");
  const [kind, setKind] = useState<engine.VocabularyKind>("drug");
  const [heard, setHeard] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);

  useEffect(() => {
    engine.vocabulary().then(setView, (e) => setError(String(e?.message ?? e)));
  }, []);

  const fail = (e: unknown) => setError(e instanceof Error ? e.message : String(e));

  async function run(change: Pending, approver: engine.Approver | null) {
    if (change.kind === "save") {
      setView(await engine.saveVocabularyTerm(change.term, approver));
      setTerm("");
      setHeard("");
      app.toast(t("{term} added", { term: change.term.term.trim() }));
    } else {
      setView(await engine.removeVocabularyTerm(change.term, approver));
      app.toast(t("{term} removed", { term: change.term }));
    }
  }

  async function ask(change: Pending) {
    setError(null);
    if (view?.needs_admin) {
      setPending(change);
      return;
    }
    setBusy(true);
    try {
      await run(change, null);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  const canEdit = !view?.needs_admin || app.user !== null;

  return (
    <section className="group">
      <h2>{t("Your clinic's words")}</h2>
      <p className="small muted measure">
        {t("Speech recognition can mishear drug names, especially brand names, accents and a laptop's microphone. When a word looks like a known name, the transcript shows a hint; it's never changed. Add the names your clinic uses, and how the transcript writes them if it gets them wrong. Changes apply from the next recording.")}
      </p>
      {view && (
        <p className="small muted measure">
          {t("Already known: {builtin} terms written for OpenBark and {regulatory} drug names from public registers (see About).", {
            builtin: view.builtin,
            regulatory: view.regulatory,
          })}
        </p>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(error)}
        </p>
      )}
      {view && view.terms.length > 0 && (
        <ul className="team-list" aria-label={t("Your clinic's words")}>
          {view.terms.map((v) => (
            <li key={v.term}>
              <span>{v.term}</span>
              <span className="small muted">
                {t(KIND_LABEL[v.kind] ?? v.kind)}
                {v.heard.length > 0 && <>. {t("Heard as {words}", { words: v.heard.join(", ") })}</>}
              </span>
              <span className="team-actions">
                <Button
                  variant="ghost"
                  size="compact"
                  disabled={busy || !canEdit}
                  aria-label={t("Remove {name}", { name: v.term })}
                  onClick={() => void ask({ kind: "remove", term: v.term })}
                >
                  {t("Remove")}
                </Button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {view && view.terms.length === 0 && <p className="small muted">{t("No words added yet.")}</p>}
      <form
        className="team-add"
        onSubmit={(e) => {
          e.preventDefault();
          void ask({ kind: "save", term: { term, kind, heard: heardWords(heard) } });
        }}
      >
        <label className="ob-field">
          <span className="label">{t("Word or name")}</span>
          <input className="ob-input" value={term} maxLength={40} onChange={(e) => setTerm(e.target.value)} placeholder="Zenrelia" />
        </label>
        <label className="ob-field">
          <span className="label">{t("Kind")}</span>
          <select className="ob-select" value={kind} onChange={(e) => setKind(e.target.value as engine.VocabularyKind)}>
            {(Object.keys(KIND_LABEL) as engine.VocabularyKind[]).map((k) => (
              <option key={k} value={k}>
                {t(KIND_LABEL[k])}
              </option>
            ))}
          </select>
        </label>
        <label className="ob-field">
          <span className="label">{t("Heard as (optional)")}</span>
          <input className="ob-input" value={heard} onChange={(e) => setHeard(e.target.value)} placeholder="zen really a" />
        </label>
        <Button type="submit" variant="primary" disabled={busy || !canEdit || !term.trim()}>
          {t("Add")}
        </Button>
      </form>
      <p className="ob-help measure">
        {t("One or two words. Heard as: the words the transcript shows instead, separated by commas, such as \"apple\" for Apoquel. For drugs, the hint appears only when the talk is about medication.")}{" "}
        {view?.needs_admin ? t("The list is the hospital's: an admin approves each change with their PIN.") : null}
      </p>
      <p className="ob-help measure">
        {t("Names are caught best with a headset or a microphone near the person speaking; a laptop's built-in microphone across the room misses more.")}
      </p>
      {pending && (
        <ApproveDialog
          title={pending.kind === "save" ? t("Add {term}", { term: pending.term.term.trim() }) : t("Remove {name}", { name: pending.term })}
          body={t("Words and drug names are shared by the whole hospital.")}
          action={pending.kind === "save" ? t("Add") : t("Remove")}
          onCancel={() => setPending(null)}
          onApprove={async (approver) => {
            await run(pending, approver);
            setPending(null);
          }}
        />
      )}
    </section>
  );
}
