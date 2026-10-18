import { Button, StatusDot } from "@openbark/ui";
import { ArrowClockwise, Copy, FilePdf, FolderOpen, Signature } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { NoteView } from "../components/NoteView";
import { PatientFields } from "../components/PatientFields";
import { DocumentSheet } from "../components/DocumentSheet";
import { HeadingEditor } from "../components/HeadingEditor";
import { RefinePanel } from "../components/RefinePanel";
import { TranscriptList } from "../components/TranscriptList";
import { fmtTime, useApp } from "../context";
import { copyToClipboard, formatNote } from "../copyFormats";
import * as engine from "../engine";
import { useT } from "../i18n";

/** DESIGN.md §6 Review: slim top bar, 2fr/3fr transcript | note, output row. */
/** `draftTemplate`: draft now with this template (from "Try on an encounter"). */
export function Review({ id, autoDraft, draftTemplate }: { id: string; autoDraft: boolean; draftTemplate?: string }) {
  const app = useApp();
  const t = useT();
  const [view, setView] = useState<engine.SessionView | null>(null);
  const [templates, setTemplates] = useState<engine.TemplateSummary[]>([]);
  const [picked, setPicked] = useState<string | null>(draftTemplate ?? null);
  /** The current version of the draft's template (for its saved standards). */
  const [current, setCurrent] = useState<engine.TemplateView | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [editingHeading, setEditingHeading] = useState<string | null>(null);
  const [savingEdit, setSavingEdit] = useState(false);
  const [addendum, setAddendum] = useState<string | null>(null);
  /** The client document open in the sheet, and the one being drafted. */
  const [openDoc, setOpenDoc] = useState<string | null>(null);
  const [drafting, setDrafting] = useState<string | null>(null);
  const [refining, setRefining] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [tokens, setTokens] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [lit, setLit] = useState<Set<number>>(new Set());
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [tab, setTab] = useState<"transcript" | "note">("note");
  const [editing, setEditing] = useState<engine.Patient | null>(null);
  const audio = useRef<HTMLAudioElement>(null);
  const [audioSrc, setAudioSrc] = useState<string | null>(null);
  const drafted = useRef(false);

  const load = useCallback(() => engine.openSession(id).then(setView, (e) => setError(String(e?.message ?? e))), [id]);

  useEffect(() => {
    void load();
    engine.listTemplates().then(setTemplates, () => {});
    const off = engine.on("token", () => setTokens((n) => n + 1));
    return () => void off.then((f) => f());
  }, [load]);

  // The recording is decrypted for playback (ADR-0008).
  const viewId = view?.id;
  useEffect(() => {
    if (!view || !viewId) return;
    let url: string | null = null;
    let live = true;
    engine.audioUrl(view).then(
      (u) => {
        url = u;
        if (live) setAudioSrc(u);
        else if (u.startsWith("blob:")) URL.revokeObjectURL(u);
      },
      () => setAudioSrc(null),
    );
    return () => {
      live = false;
      if (url?.startsWith("blob:")) URL.revokeObjectURL(url);
    };
    // Only refetch when the encounter changes, not on every view refresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewId]);

  const segments = view?.transcript?.segments ?? [];
  const run = view?.latest_run ?? null;
  // Notes render under the headings they were drafted to (saved with the draft).
  const template = view?.latest_template ?? null;
  const chosen = picked ?? template?.id ?? app.settings.template;

  useEffect(() => {
    if (!template) return;
    engine.template(template.id).then(setCurrent, () => setCurrent(null));
  }, [template]);

  const draft = useCallback(async () => {
    setError(null);
    setTokens(0);
    setGenerating(true);
    try {
      await engine.generateNote(id, app.settings.llm, chosen, app.settings.noteLanguage);
      await load();
      app.announce(t("Draft ready"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setGenerating(false);
    }
  }, [id, app, load, chosen, t]);

  async function draftDocument(templateId: string) {
    setError(null);
    setTokens(0);
    setDrafting(templateId);
    setOpenDoc(templateId);
    try {
      await engine.generateNote(id, app.settings.llm, templateId, app.settings.noteLanguage);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setOpenDoc(null);
    } finally {
      setDrafting(null);
    }
  }

  async function saveEdit(heading: string, value: unknown) {
    if (!template) return;
    if (!app.settings.signer.trim()) {
      setError(t("Add your name in Settings, then save your edits."));
      return;
    }
    setError(null);
    setSavingEdit(true);
    try {
      await engine.editNote(id, heading, value, app.settings.signer);
      await load();
      setEditingHeading(null);
      app.toast(t("{heading} saved", { heading: engine.headingTitle(template, heading) }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingEdit(false);
    }
  }

  async function signAddendum() {
    if (!addendum?.trim()) return;
    if (!app.settings.signer.trim()) {
      setError(t("Add your name in Settings, then sign the addendum."));
      return;
    }
    try {
      setView(await engine.addAddendum(id, addendum, app.settings.signer));
      setAddendum(null);
      app.toast(t("Addendum signed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function refine(heading: string, request: string, saveAsStandard: boolean) {
    if (!template) return;
    setError(null);
    setTokens(0);
    setRefining(true);
    try {
      await engine.refineNote(id, {
        llm: app.settings.llm,
        heading,
        request,
        save_as_standard: saveAsStandard,
        by: app.settings.signer.trim() || null,
        language: app.settings.noteLanguage,
      });
      await load();
      setAsking(null);
      const title = engine.headingTitle(template, heading);
      app.toast(saveAsStandard ? t("{heading} redrafted and saved as a standard", { heading: title }) : t("{heading} redrafted", { heading: title }));
      app.announce(t("{heading} redrafted", { heading: title }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRefining(false);
    }
  }

  useEffect(() => {
    const wanted = draftTemplate != null || (autoDraft && !view?.latest_run);
    if (wanted && view && segments.length > 0 && !drafted.current) {
      drafted.current = true;
      void draft();
    }
  }, [autoDraft, draftTemplate, view, segments.length, draft]);

  function playIds(ids: number[]) {
    const first = segments.find((s) => ids.includes(s.id));
    if (first) seek(first);
  }

  function seek(s: engine.Segment) {
    const a = audio.current;
    if (!a) return;
    a.currentTime = s.start;
    void a.play();
  }

  function onTime() {
    const now = audio.current?.currentTime ?? 0;
    const cur = segments.find((s) => now >= s.start && now < s.end + 0.3);
    setPlayingId(cur?.id ?? null);
  }

  async function copy(text: string, what: string, html?: string) {
    try {
      await copyToClipboard({ text, html });
      app.toast(t("{what} copied", { what }));
    } catch {
      setError(t("The clipboard isn't available here. Select the text and copy it instead."));
    }
  }

  /** Lines above the note when it's copied: patient, reason, recorded, signed. */
  function headerLines(): string[] {
    const lines: string[] = [];
    const pt = view?.patient;
    if (pt && `${pt.name} ${pt.last_name}`.trim()) lines.push(`${t("Patient")}: ${`${pt.name} ${pt.last_name}`.trim()}`);
    if (pt?.reason) lines.push(`${t("Reason for visit")}: ${pt.reason}`);
    if (view) lines.push(`${t("Recorded")}: ${view.created_at}`);
    if (view?.signed_at) lines.push(t("Signed by {name}, {at}", { name: view.signed_by ?? "", at: view.signed_at }));
    return lines;
  }

  async function copyNote(tpl: engine.Template, noteRun: engine.NoteRun, what: string, withAddenda: boolean) {
    const out = formatNote(tpl, noteRun, {
      format: app.settings.copyFormat,
      citations: app.settings.copyCitations,
      header: headerLines(),
      addenda: withAddenda
        ? (view?.addenda ?? []).map((a) => [`${t("Addendum")}, ${engine.fmtStamp(a.at)}, ${a.by}`, a.text] as [string, string])
        : [],
      notDiscussed: t("Not discussed"),
    });
    await copy(out.text, what, out.html);
  }

  async function savePdf(document?: string) {
    setError(null);
    try {
      const saved = await engine.exportPdf(id, {
        clinic: { name: app.settings.clinicName, lines: app.settings.clinicLines.split("\n").filter((l) => l.trim()) },
        page: app.settings.pageSize,
        document: document ?? null,
        language: app.settings.noteLanguage,
      });
      if (saved) app.toast(t("PDF saved"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function sign() {
    if (!app.settings.signer.trim()) {
      setError(t("Add your name in Settings, then sign."));
      return;
    }
    try {
      setView(await engine.signNote(id, app.settings.signer));
      app.toast(t("Note signed"));
      app.announce(t("Note signed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function savePatient() {
    if (!editing) return;
    try {
      setView(await engine.updatePatient(id, editing));
      setEditing(null);
      app.toast(t("Patient details saved"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const p = view?.patient ?? null;
  const patientName = p ? `${p.name} ${p.last_name}`.trim() : "";
  const hardFails = run?.faithfulness.unsupported_numbers.length ?? 0;
  const softFlags =
    (run?.faithfulness.citation_mismatches.length ?? 0) +
    (run?.faithfulness.invalid_evidence.length ?? 0) +
    (run?.faithfulness.unverified_values?.length ?? 0);
  const disputed = run?.faithfulness.disputed_numbers?.length ?? 0;
  const signed = Boolean(view?.signed_at);
  const progress = Math.min(0.95, tokens / 1400);

  return (
    <div className="screen review">
      <header className="review-bar">
        <div className="review-title">
          <h1>{patientName || (view ? view.created_at : t("Encounter"))}</h1>
          {view?.stats && <span className="mono faint">{fmtTime(view.stats.audio_secs)}</span>}
        </div>
        <div className="review-actions">
          {signed || templates.length === 0 ? (
            <span className="ob-chip">{template?.name ?? t("No draft")}</span>
          ) : (
            <select
              className="ob-select template-pick"
              aria-label={t("Template for the next draft")}
              value={chosen}
              disabled={generating || refining}
              onChange={(e) => setPicked(e.target.value)}
            >
              {templates
                .filter((x) => x.document !== "client")
                .map((x) => (
                  <option key={x.id} value={x.id}>
                    {x.name}
                  </option>
                ))}
            </select>
          )}
          {run && <span className="ob-chip mono">{run.model}</span>}
          {signed ? (
            <StatusDot tone="ok">{t("Signed by {name}, {at}", { name: view?.signed_by ?? "", at: view?.signed_at ?? "" })}</StatusDot>
          ) : run ? (
            <Button variant="primary" onClick={sign} disabled={generating || refining || hardFails > 0}>
              <Signature size={20} aria-hidden />
              {t("Sign note")}
            </Button>
          ) : (
            <Button variant="primary" onClick={draft} disabled={generating || segments.length === 0}>
              {t("Draft note")}
            </Button>
          )}
        </div>
      </header>

      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <section className="visit" aria-label={t("Visit details")}>
        {editing ? (
          <form
            className="visit-edit"
            onSubmit={(e) => {
              e.preventDefault();
              void savePatient();
            }}
          >
            <PatientFields value={editing} onChange={setEditing} idPrefix="edit-patient" />
            <div className="visit-actions">
              <Button type="submit">{t("Save details")}</Button>
              <Button variant="ghost" onClick={() => setEditing(null)}>
                {t("Cancel")}
              </Button>
            </div>
          </form>
        ) : (
          <dl className="visit-facts">
            <div>
              <dt className="label muted">{t("Reason for visit")}</dt>
              <dd>{p?.reason || <span className="faint">{t("Not entered")}</span>}</dd>
            </div>
            <div>
              <dt className="label muted">{t("Recorded")}</dt>
              <dd className="mono">{view?.created_at}</dd>
            </div>
            <div>
              <dt className="label muted">{t("Consent to record")}</dt>
              <dd>{view?.consent ? engine.consentText(view.consent) : <span className="faint">{t("Not recorded")}</span>}</dd>
            </div>
            {p?.entered_at && (
              <div>
                <dt className="label muted">{t("Details entered")}</dt>
                <dd className="mono">
                  {engine.fmtStamp(p.entered_at)}
                  {p.updated_at ? `, ${t("changed")} ${engine.fmtStamp(p.updated_at)}` : ""}
                </dd>
              </div>
            )}
            <div className="visit-edit-btn">
              <Button variant="ghost" onClick={() => setEditing(p ? { ...p } : engine.emptyPatient())} disabled={!view}>
                {p ? t("Edit details") : t("Add patient details")}
              </Button>
            </div>
          </dl>
        )}
      </section>

      <div className="review-tabs ob-segmented" role="tablist">
        <button className="ob-segment" role="tab" aria-selected={tab === "transcript"} onClick={() => setTab("transcript")}>
          {t("Transcript")}
        </button>
        <button className="ob-segment" role="tab" aria-selected={tab === "note"} onClick={() => setTab("note")}>
          {t("Note")}
        </button>
      </div>

      <div className="review-split" data-tab={tab}>
        <section className="pane pane-transcript" aria-label={t("Transcript")}>
          {view && audioSrc && (
            <audio
              ref={audio}
              className="player"
              controls
              preload="metadata"
              src={audioSrc}
              onTimeUpdate={onTime}
              onPause={() => setPlayingId(null)}
            />
          )}
          {segments.length > 0 ? (
            <TranscriptList segments={segments} highlighted={lit} playingId={playingId} onSeek={seek} />
          ) : (
            <div className="pane-pad">
              <div className="ob-skeleton" style={{ height: 16, width: "70%" }} />
              <div className="ob-skeleton" style={{ height: 16, width: "55%", marginTop: 12 }} />
            </div>
          )}
        </section>

        <section className="pane pane-note" aria-label={t("Note")}>
          {generating && (
            <div className="ob-progress" role="progressbar" aria-label={t("Drafting the note")}>
              <span style={{ transform: `scaleX(${progress})` }} />
            </div>
          )}
          {run && template ? (
            <>
              {(hardFails > 0 || softFlags > 0 || disputed > 0) && (
                <p className="ob-notice note-summary" data-tone={hardFails ? "danger" : undefined}>
                  {hardFails > 0 &&
                    `${hardFails === 1 ? t("1 number isn't in the transcript.") : t("{n} numbers aren't in the transcript.", { n: hardFails })} `}
                  {disputed > 0 &&
                    `${disputed === 1 ? t("1 number was heard differently by the second recogniser; listen before signing.") : t("{n} numbers were heard differently by the second recogniser; listen before signing.", { n: disputed })} `}
                  {softFlags > 0 &&
                    `${softFlags === 1 ? t("1 item needs checking against the transcript.") : t("{n} items need checking against the transcript.", { n: softFlags })} `}
                  {t("Rest on a sentence to see its evidence.")}
                </p>
              )}
              <NoteView
                template={template}
                run={run}
                onEvidence={(ids) => setLit(new Set(ids ?? []))}
                onPlay={playIds}
                onCopySection={(title, text) => copy(`${title}\n${text}`, title)}
                onAsk={
                  signed || generating
                    ? undefined
                    : (heading) => {
                        setEditingHeading(null);
                        setAsking((a) => (a === heading ? null : heading));
                      }
                }
                onEdit={
                  signed || generating || refining
                    ? undefined
                    : (heading) => {
                        setAsking(null);
                        setEditingHeading((h) => (h === heading ? null : heading));
                      }
                }
                renderEdit={(heading) => {
                  if (editingHeading !== heading) return null;
                  const def = headingDef(template, heading);
                  if (!def) return null;
                  const [sec, part] = heading.split("/");
                  const sectionValue = run.note[sec ?? ""] as Record<string, unknown> | undefined;
                  return (
                    <HeadingEditor
                      title={engine.headingTitle(template, heading)}
                      kind={def.kind}
                      fields={def.fields}
                      value={part ? sectionValue?.[part] : sectionValue}
                      busy={savingEdit}
                      onSave={(value) => void saveEdit(heading, value)}
                      onCancel={() => setEditingHeading(null)}
                    />
                  );
                }}
                renderAsk={(heading) =>
                  asking === heading && (
                    <RefinePanel
                      title={engine.headingTitle(template, heading)}
                      templateName={template.name}
                      standards={(current?.template.standards ?? []).filter((s) => s.heading === heading)}
                      canSave={current != null}
                      busy={refining}
                      progress={Math.min(0.95, tokens / 400)}
                      onSubmit={(request, save) => void refine(heading, request, save)}
                      onCancel={() => setAsking(null)}
                    />
                  )
                }
              />
              <footer className="output-row">
                <Button onClick={() => void copyNote(template, run, t("Note"), true)}>
                  <Copy size={20} aria-hidden />
                  {t("Copy note")}
                </Button>
                <Button variant="ghost" onClick={() => void savePdf()}>
                  <FilePdf size={20} aria-hidden />
                  {t("Save PDF")}
                </Button>
                {!signed && (
                  <Button variant="ghost" onClick={draft} disabled={generating || refining}>
                    <ArrowClockwise size={20} aria-hidden />
                    {chosen === template?.id
                      ? t("Draft again")
                      : t("Draft as {name}", { name: templates.find((x) => x.id === chosen)?.name ?? chosen })}
                  </Button>
                )}
                {templates
                  .filter((x) => x.document === "client")
                  .map((x) => {
                    const has = view?.documents.some((d) => d.template_id === x.id);
                    return (
                      <Button
                        key={x.id}
                        variant="ghost"
                        disabled={drafting != null || generating}
                        onClick={() => (has ? setOpenDoc(x.id) : void draftDocument(x.id))}
                      >
                        {x.name}
                      </Button>
                    );
                  })}
                {engine.canReveal && view && (
                  <Button variant="ghost" onClick={() => engine.reveal(view.dir)}>
                    <FolderOpen size={20} aria-hidden />
                    {t("Show files")}
                  </Button>
                )}
                <span className="small faint output-meta">
                  {t("{statements} statements, {numbers} numbers checked against the transcript.", {
                    statements: run.faithfulness.statements_checked,
                    numbers: run.faithfulness.numbers_checked,
                  })}
                  {(run.faithfulness.vet_entered?.length ?? 0) > 0 &&
                    ` ${t("{n} entered by a veterinarian.", { n: run.faithfulness.vet_entered?.length ?? 0 })}`}
                  {run.stats.total_ms > 0 && ` ${t("Drafted in {s} s.", { s: (run.stats.total_ms / 1000).toFixed(1) })}`}
                </span>
              </footer>
              {signed && (
                <section className="addenda" aria-label={t("Addenda")}>
                  {(view?.addenda ?? []).map((a) => (
                    <article key={a.id} className="addendum">
                      <h3 className="label">
                        {t("Addendum")}, <span className="mono">{engine.fmtStamp(a.at)}</span>, {a.by}
                      </h3>
                      <p>{a.text}</p>
                    </article>
                  ))}
                  {addendum === null ? (
                    <Button variant="ghost" onClick={() => setAddendum("")}>
                      {t("Add addendum")}
                    </Button>
                  ) : (
                    <form
                      className="edit-heading"
                      onSubmit={(e) => {
                        e.preventDefault();
                        void signAddendum();
                      }}
                    >
                      <label className="label" htmlFor="addendum-text">
                        {t("Addendum")}
                      </label>
                      <textarea
                        id="addendum-text"
                        className="ob-input tb-text"
                        rows={3}
                        autoFocus
                        value={addendum}
                        onChange={(e) => setAddendum(e.target.value)}
                        placeholder={t("For example: owner called on day 2, eating again, vomiting stopped.")}
                      />
                      <p className="small faint">{t("The signed note doesn't change. The addendum is dated and signed with your name.")}</p>
                      <div className="ask-actions">
                        <Button type="submit" variant="primary" disabled={!addendum.trim()}>
                          {t("Sign addendum")}
                        </Button>
                        <Button variant="ghost" onClick={() => setAddendum(null)}>
                          {t("Cancel")}
                        </Button>
                      </div>
                    </form>
                  )}
                </section>
              )}
            </>
          ) : (
            <div className="pane-pad">
              {generating ? (
                <p className="small muted">{t("Writing the draft. Numbers are checked against the transcript when it finishes.")}</p>
              ) : (
                <p className="small muted">{t("No draft yet. Draft the note to see it here.")}</p>
              )}
            </div>
          )}
        </section>
      </div>
      {openDoc && (
        <DocumentSheet
          doc={
            view?.documents.find((d) => d.template_id === openDoc) ?? {
              template_id: openDoc,
              name: templates.find((t) => t.id === openDoc)?.name ?? openDoc,
              created_at: "",
              run: null,
              markdown: null,
              template: null,
            }
          }
          busy={drafting === openDoc}
          progress={Math.min(0.95, tokens / 900)}
          onCopy={(doc) =>
            doc.template && doc.run
              ? void copyNote(doc.template, doc.run, doc.name, false)
              : undefined
          }
          onPdf={() => void savePdf(openDoc)}
          onRedraft={() => void draftDocument(openDoc)}
          onClose={() => setOpenDoc(null)}
          onEvidence={(ids) => setLit(new Set(ids ?? []))}
          onPlay={playIds}
        />
      )}
    </div>
  );
}

/** What goes under a heading path, from the draft's template. */
function headingDef(t: engine.Template, heading: string): { kind: engine.ContentKind; fields?: engine.TemplateField[] } | null {
  const [sec, part] = heading.split("/");
  const s = t.sections.find((x) => x.id === sec);
  if (!s) return null;
  if (!part) return s.kind ? { kind: s.kind, fields: s.fields } : null;
  const p = s.parts?.find((x) => x.key === part);
  return p ? { kind: p.kind, fields: p.fields } : null;
}
