import { Button, MarkMono, StatusDot } from "@openbark/ui";
import { ArrowLeft, Mic, Pencil, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { ReprocessSheet } from "../components/ReprocessSheet";
import { SPECIES, speciesName } from "../species";
import { SpeciesIcon, TemplateIcon } from "../templateIcons";

/** One patient (ADR-0011): their details, and every visit, newest first. */
export function PatientDetail({ id }: { id: string }) {
  const app = useApp();
  const t = useT();
  const [view, setView] = useState<engine.PatientView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [form, setForm] = useState({ name: "", last_name: "", species: "" });
  const [busy, setBusy] = useState(false);
  /** The visit whose note is being drafted again (Reprocess). */
  const [reprocess, setReprocess] = useState<engine.VisitSummary | null>(null);

  useEffect(() => {
    engine.patientView(id).then(setView, (e) => setError(String(e?.message ?? e)));
  }, [id]);

  async function save() {
    setBusy(true);
    setError(null);
    try {
      const patient = await engine.updatePatientRecord(id, form.name, form.last_name, form.species);
      setView((v) => (v ? { ...v, patient } : v));
      setEditing(false);
      app.toast(t("Patient details saved"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const p = view?.patient;
  /** This patient's visits in Deleted visits (ADR-0022 section 11). */
  const deleted = view?.deleted ?? 0;
  const deletedLine =
    deleted > 0 ? (
      <p className="small muted deleted-line">
        {deleted === 1
          ? t("1 deleted visit is kept for 30 days. Restore it from Deleted visits in Settings.")
          : t("{n} deleted visits are kept for 30 days. Restore them from Deleted visits in Settings.", { n: deleted })}
      </p>
    ) : null;
  return (
    <div className="screen patient-page">
      <header className="screen-head">
        <div className="patient-title">
          <button type="button" className="link small back-link" onClick={() => app.navigate({ screen: "patients" })}>
            <ArrowLeft size={16} aria-hidden />
            {t("Patients")}
          </button>
          <div className="patient-heading">
            <span className="patient-avatar large" aria-hidden>
              <SpeciesIcon species={p?.species} size={28} />
            </span>
            <div>
              <h1>
                {p?.name ?? ""}
                {p?.last_name && <span className="muted"> {p.last_name}</span>}
              </h1>
              <span className="small muted">
                {[
                  p?.species ? t(speciesName(p.species)) : t("Species not entered"),
                  view ? (view.visits.length === 1 ? t("1 visit") : t("{n} visits", { n: view.visits.length })) : "",
                ]
                  .filter(Boolean)
                  .join(", ")}
              </span>
            </div>
          </div>
        </div>
        <div className="head-actions">
          {p && !editing && (
            <Button
              variant="ghost"
              onClick={() => {
                setForm({ name: p.name, last_name: p.last_name, species: p.species });
                setEditing(true);
              }}
            >
              <Pencil size={18} aria-hidden />
              {t("Edit details")}
            </Button>
          )}
          <Button variant="primary" onClick={() => app.navigate({ screen: "record", patient: id })} disabled={!p}>
            <Mic size={20} aria-hidden />
            {t("New visit")}
          </Button>
        </div>
      </header>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {editing && (
        <form
          className="group patient-edit"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="patient-edit-fields">
            <label className="ob-field">
              <span className="label">{t("Patient name")}</span>
              <input
                className="ob-input"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
            <label className="ob-field">
              <span className="label">{t("Last name")}</span>
              <input
                className="ob-input"
                value={form.last_name}
                onChange={(e) => setForm({ ...form, last_name: e.target.value })}
              />
            </label>
            <label className="ob-field">
              <span className="label">{t("Species")}</span>
              <select
                className="ob-select"
                value={form.species}
                onChange={(e) => setForm({ ...form, species: e.target.value })}
              >
                <option value="">{t("Not entered")}</option>
                {SPECIES.map((s) => (
                  <option key={s.key} value={s.key}>
                    {t(s.one)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <p className="small muted">{t("Past visits keep the details entered at the time.")}</p>
          <div className="ask-actions">
            <Button type="submit" variant="primary" disabled={busy || !form.name.trim()}>
              {t("Save details")}
            </Button>
            <Button variant="ghost" onClick={() => setEditing(false)} disabled={busy}>
              {t("Cancel")}
            </Button>
          </div>
        </form>
      )}
      {view === null ? (
        <div className="ob-skeleton" style={{ height: 160 }} />
      ) : view.visits.length === 0 ? (
        <div className="empty-state">
          <MarkMono size={48} className="faint" />
          <h3>{t("No visits yet")}</h3>
          <p className="muted">{t("Record the first visit for this patient.")}</p>
          <Button variant="primary" onClick={() => app.navigate({ screen: "record", patient: id })} disabled={!p}>
            {t("New visit")}
          </Button>
          {deletedLine}
        </div>
      ) : (
        <section aria-label={t("Visits")}>
          <h2 className="tpl-group-title">{t("Visits")}</h2>
          <ul className="visit-list">
            {view.visits.map((v) => (
              <li key={v.id} className="visit-item">
                <button
                  type="button"
                  className="visit-row"
                  onClick={() => app.navigate({ screen: "review", id: v.id, pet: v.pet ?? undefined })}
                >
                  <span className="type-icon">
                    <TemplateIcon template={{ icon: v.template_icon }} size={20} />
                  </span>
                  <span className="visit-text">
                    <span className="visit-type">{v.template ? templateName(t, v.template) : t("No draft yet")}</span>
                    <span className="small muted visit-reason">
                      {v.reason || t("No reason entered")}
                      {v.pet && ` (${t("visit with other animals")})`}
                    </span>
                  </span>
                  <span className="visit-meta">
                    <span className="small faint mono">{v.created_at}</span>
                    <span className="small faint mono">{fmtTime(v.audio_secs)}</span>
                    {v.signed ? (
                      <StatusDot tone="ok">{t("Signed")}</StatusDot>
                    ) : v.drafted ? (
                      <StatusDot tone="warn">{t("Draft")}</StatusDot>
                    ) : (
                      <StatusDot>{t("Transcribed")}</StatusDot>
                    )}
                  </span>
                </button>
                <button
                  type="button"
                  className="icon-btn reprocess-btn"
                  aria-label={t("Reprocess this visit")}
                  title={
                    v.signed
                      ? t("Signed notes are final. Add an addendum in Review instead.")
                      : !v.drafted
                        ? t("Draft this visit")
                        : t("Reprocess this visit")
                  }
                  disabled={v.signed || !app.can.notes}
                  onClick={() => setReprocess(v)}
                >
                  <RefreshCw size={18} aria-hidden />
                </button>
              </li>
            ))}
          </ul>
          {deletedLine}
        </section>
      )}
      {reprocess && (
        <ReprocessSheet
          visit={reprocess}
          onClose={() => setReprocess(null)}
          onChoose={(template) =>
            app.navigate({ screen: "review", id: reprocess.id, draft: true, template, pet: reprocess.pet ?? undefined })
          }
        />
      )}
    </div>
  );
}
