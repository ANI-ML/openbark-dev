import { Button, StatusDot } from "@openbark/ui";
import { ArrowClockwise, Copy, FolderOpen, Signature } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { NoteView } from "../components/NoteView";
import { PatientFields } from "../components/PatientFields";
import { HeadingEditor } from "../components/HeadingEditor";
import { RefinePanel } from "../components/RefinePanel";
import { TranscriptList } from "../components/TranscriptList";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";

/** DESIGN.md §6 Review: slim top bar, 2fr/3fr transcript | note, output row. */
/** `draftTemplate`: draft now with this template (from "Try on an encounter"). */
export function Review({ id, autoDraft, draftTemplate }: { id: string; autoDraft: boolean; draftTemplate?: string }) {
  const app = useApp();
  const [view, setView] = useState<engine.SessionView | null>(null);
  const [templates, setTemplates] = useState<engine.TemplateSummary[]>([]);
  const [picked, setPicked] = useState<string | null>(draftTemplate ?? null);
  /** The current version of the draft's template (for its saved standards). */
  const [current, setCurrent] = useState<engine.TemplateView | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [editingHeading, setEditingHeading] = useState<string | null>(null);
  const [savingEdit, setSavingEdit] = useState(false);
  const [addendum, setAddendum] = useState<string | null>(null);
  const [refining, setRefining] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [tokens, setTokens] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [lit, setLit] = useState<Set<number>>(new Set());
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [tab, setTab] = useState<"transcript" | "note">("note");
  const [editing, setEditing] = useState<engine.Patient | null>(null);
  const audio = useRef<HTMLAudioElement>(null);
  const drafted = useRef(false);

  const load = useCallback(() => engine.openSession(id).then(setView, (e) => setError(String(e?.message ?? e))), [id]);

  useEffect(() => {
    void load();
    engine.listTemplates().then(setTemplates, () => {});
    const off = engine.on("token", () => setTokens((n) => n + 1));
    return () => void off.then((f) => f());
  }, [load]);

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
      await engine.generateNote(id, app.settings.llm, chosen);
      await load();
      app.announce("Draft ready");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setGenerating(false);
    }
  }, [id, app, load, chosen]);

  async function saveEdit(heading: string, value: unknown) {
    if (!template) return;
    if (!app.settings.signer.trim()) {
      setError("Add your name in Settings, then save your edits.");
      return;
    }
    setError(null);
    setSavingEdit(true);
    try {
      await engine.editNote(id, heading, value, app.settings.signer);
      await load();
      setEditingHeading(null);
      app.toast(`${engine.headingTitle(template, heading)} saved`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingEdit(false);
    }
  }

  async function signAddendum() {
    if (!addendum?.trim()) return;
    if (!app.settings.signer.trim()) {
      setError("Add your name in Settings, then sign the addendum.");
      return;
    }
    try {
      setView(await engine.addAddendum(id, addendum, app.settings.signer));
      setAddendum(null);
      app.toast("Addendum signed");
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
      });
      await load();
      setAsking(null);
      const title = engine.headingTitle(template, heading);
      app.toast(saveAsStandard ? `${title} redrafted and saved as a standard` : `${title} redrafted`);
      app.announce(`${title} redrafted`);
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
    const t = audio.current?.currentTime ?? 0;
    const cur = segments.find((s) => t >= s.start && t < s.end + 0.3);
    setPlayingId(cur?.id ?? null);
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      app.toast(`${what} copied`);
    } catch {
      setError("The clipboard isn't available here. Select the text and copy it instead.");
    }
  }

  async function sign() {
    if (!app.settings.signer.trim()) {
      setError("Add your name in Settings, then sign.");
      return;
    }
    try {
      setView(await engine.signNote(id, app.settings.signer));
      app.toast("Note signed");
      app.announce("Note signed");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function savePatient() {
    if (!editing) return;
    try {
      setView(await engine.updatePatient(id, editing));
      setEditing(null);
      app.toast("Patient details saved");
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
          <h1>{patientName || (view ? view.created_at : "Encounter")}</h1>
          {view?.stats && <span className="mono faint">{fmtTime(view.stats.audio_secs)}</span>}
        </div>
        <div className="review-actions">
          {signed || templates.length === 0 ? (
            <span className="ob-chip">{template?.name ?? "No draft"}</span>
          ) : (
            <select
              className="ob-select template-pick"
              aria-label="Template for the next draft"
              value={chosen}
              disabled={generating || refining}
              onChange={(e) => setPicked(e.target.value)}
            >
              {templates.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
          )}
          {run && <span className="ob-chip mono">{run.model}</span>}
          {signed ? (
            <StatusDot tone="ok">
              Signed by {view?.signed_by}, {view?.signed_at}
            </StatusDot>
          ) : run ? (
            <Button variant="primary" onClick={sign} disabled={generating || refining || hardFails > 0}>
              <Signature size={20} aria-hidden />
              Sign note
            </Button>
          ) : (
            <Button variant="primary" onClick={draft} disabled={generating || segments.length === 0}>
              Draft note
            </Button>
          )}
        </div>
      </header>

      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <section className="visit" aria-label="Visit details">
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
              <Button type="submit">Save details</Button>
              <Button variant="ghost" onClick={() => setEditing(null)}>
                Cancel
              </Button>
            </div>
          </form>
        ) : (
          <dl className="visit-facts">
            <div>
              <dt className="label muted">Reason for visit</dt>
              <dd>{p?.reason || <span className="faint">Not entered</span>}</dd>
            </div>
            <div>
              <dt className="label muted">Recorded</dt>
              <dd className="mono">{view?.created_at}</dd>
            </div>
            {p?.entered_at && (
              <div>
                <dt className="label muted">Details entered</dt>
                <dd className="mono">
                  {engine.fmtStamp(p.entered_at)}
                  {p.updated_at ? `, changed ${engine.fmtStamp(p.updated_at)}` : ""}
                </dd>
              </div>
            )}
            <div className="visit-edit-btn">
              <Button variant="ghost" onClick={() => setEditing(p ? { ...p } : engine.emptyPatient())} disabled={!view}>
                {p ? "Edit details" : "Add patient details"}
              </Button>
            </div>
          </dl>
        )}
      </section>

      <div className="review-tabs ob-segmented" role="tablist">
        <button className="ob-segment" role="tab" aria-selected={tab === "transcript"} onClick={() => setTab("transcript")}>
          Transcript
        </button>
        <button className="ob-segment" role="tab" aria-selected={tab === "note"} onClick={() => setTab("note")}>
          Note
        </button>
      </div>

      <div className="review-split" data-tab={tab}>
        <section className="pane pane-transcript" aria-label="Transcript">
          {view && (
            <audio
              ref={audio}
              className="player"
              controls
              preload="metadata"
              src={engine.audioSrc(view)}
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

        <section className="pane pane-note" aria-label="Note">
          {generating && (
            <div className="ob-progress" role="progressbar" aria-label="Drafting the note">
              <span style={{ transform: `scaleX(${progress})` }} />
            </div>
          )}
          {run && template ? (
            <>
              {(hardFails > 0 || softFlags > 0 || disputed > 0) && (
                <p className="ob-notice note-summary" data-tone={hardFails ? "danger" : undefined}>
                  {hardFails > 0 && `${hardFails} number${hardFails === 1 ? " isn't" : "s aren't"} in the transcript. `}
                  {disputed > 0 && `${disputed} number${disputed === 1 ? " was" : "s were"} heard differently by the second recogniser; listen before signing. `}
                  {softFlags > 0 &&
                    `${softFlags} ${softFlags === 1 ? "item needs" : "items need"} checking against the transcript. `}
                  Rest on a sentence to see its evidence.
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
                <Button onClick={() => copy(view?.latest_markdown ?? "", "Note")}>
                  <Copy size={20} aria-hidden />
                  Copy note
                </Button>
                {!signed && (
                  <Button variant="ghost" onClick={draft} disabled={generating || refining}>
                    <ArrowClockwise size={20} aria-hidden />
                    {chosen === template?.id ? "Draft again" : `Draft as ${templates.find((t) => t.id === chosen)?.name ?? chosen}`}
                  </Button>
                )}
                {engine.canReveal && view && (
                  <Button variant="ghost" onClick={() => engine.reveal(view.dir)}>
                    <FolderOpen size={20} aria-hidden />
                    Show files
                  </Button>
                )}
                <span className="small faint output-meta">
                  {run.faithfulness.statements_checked} statements, {run.faithfulness.numbers_checked} numbers checked
                  against the transcript.
                  {(run.faithfulness.vet_entered?.length ?? 0) > 0 && ` ${run.faithfulness.vet_entered?.length} entered by a veterinarian.`}
                  {run.stats.total_ms > 0 && ` Drafted in ${(run.stats.total_ms / 1000).toFixed(1)} s.`}
                </span>
              </footer>
              {signed && (
                <section className="addenda" aria-label="Addenda">
                  {(view?.addenda ?? []).map((a) => (
                    <article key={a.id} className="addendum">
                      <h3 className="label">
                        Addendum, <span className="mono">{engine.fmtStamp(a.at)}</span>, {a.by}
                      </h3>
                      <p>{a.text}</p>
                    </article>
                  ))}
                  {addendum === null ? (
                    <Button variant="ghost" onClick={() => setAddendum("")}>
                      Add addendum
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
                        Addendum
                      </label>
                      <textarea
                        id="addendum-text"
                        className="ob-input tb-text"
                        rows={3}
                        autoFocus
                        value={addendum}
                        onChange={(e) => setAddendum(e.target.value)}
                        placeholder="For example: owner called on day 2, eating again, vomiting stopped."
                      />
                      <p className="small faint">The signed note doesn't change. The addendum is dated and signed with your name.</p>
                      <div className="ask-actions">
                        <Button type="submit" variant="primary" disabled={!addendum.trim()}>
                          Sign addendum
                        </Button>
                        <Button variant="ghost" onClick={() => setAddendum(null)}>
                          Cancel
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
                <p className="small muted">Writing the draft. Numbers are checked against the transcript when it finishes.</p>
              ) : (
                <p className="small muted">No draft yet. Draft the note to see it here.</p>
              )}
            </div>
          )}
        </section>
      </div>
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
