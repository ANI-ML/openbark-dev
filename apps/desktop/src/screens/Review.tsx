import { Button, MarkMono, Sheet, Skeleton, StatusDot } from "@openbark/ui";
import { Copy, FileDown, FolderOpen, RotateCw, Signature, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { DeleteRecordingDialog } from "../components/DeleteRecordingDialog";
import { DeleteVisitMenu } from "../components/DeleteVisitMenu";
import { listNames, patientIdFor } from "../deleting";
import { NoteView } from "../components/NoteView";
import { PatientFields } from "../components/PatientFields";
import { DocumentSheet } from "../components/DocumentSheet";
import { DraftBoard, useDraftTracker } from "../components/DraftProgress";
import { HeadingEditor } from "../components/HeadingEditor";
import { DentalChartEditor } from "../components/DentalChartEditor";
import { asItems, dentitionOf, useLowerInner, type Dentition } from "../dental";
import { toothFlagPaths } from "../components/DentalChart";
import { RefinePanel } from "../components/RefinePanel";
import { headingExamples } from "../promptExamples";
import { speciesName } from "../species";
import { TranscriptList } from "../components/TranscriptList";
import { CallButton } from "../components/CallButton";
import { callLength, formatPhone } from "../phone/format";
import { fmtTime, useApp } from "../context";
import { copyToClipboard, formatNote } from "../copyFormats";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { draftLabel } from "../templateLabels";

/**
 * Review: slim top bar, transcript | note split 5:7 on desktop
 * (docs/brand/DESIGN.md §7.5, §9), output row. The model chip says where
 * the note was written (§7.6), and the empty note follows §7.7.
 * `draftTemplate`: draft now with this template (from "Try on a visit").
 */
export function Review({
  id,
  autoDraft,
  draftTemplate,
  initialPet,
}: {
  id: string;
  autoDraft: boolean;
  draftTemplate?: string;
  /** In a multi-pet visit, the animal to show first (opened from its patient page). */
  initialPet?: string;
}) {
  const app = useApp();
  const t = useT();
  const lowerInner = useLowerInner();
  const [view, setView] = useState<engine.SessionView | null>(null);
  const [templates, setTemplates] = useState<engine.TemplateSummary[]>([]);
  const [picked, setPicked] = useState<string | null>(draftTemplate ?? null);
  /** The current version of the draft's template (for its saved standards). */
  const [current, setCurrent] = useState<engine.TemplateView | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [editingHeading, setEditingHeading] = useState<string | null>(null);
  /** The tooth clicked to open the chart editor (ADR-0021 §8). */
  const [editingTeeth, setEditingTeeth] = useState<string[]>([]);
  const [savingEdit, setSavingEdit] = useState(false);
  /** A tooth chart confirmation being saved (ADR-0021 decision 10). */
  const [confirming, setConfirming] = useState(false);
  const [addendum, setAddendum] = useState<string | null>(null);
  /** The client document open in the sheet, and the one being drafted. */
  const [openDoc, setOpenDoc] = useState<string | null>(null);
  const [drafting, setDrafting] = useState<string | null>(null);
  /** The client's language for client documents (M2 item 6). */
  const [docLanguage, setDocLanguage] = useState<string>(app.settings.noteLanguage);
  const [refining, setRefining] = useState(false);
  const [generating, setGenerating] = useState(false);
  /** The drafting board: what's being drafted, with which template's headings. */
  const tracker = useDraftTracker();
  const [draftingWith, setDraftingWith] = useState<engine.Template | null>(null);
  /** A multi-pet visit drafted one animal after another: which one, of how many. */
  const [petRound, setPetRound] = useState<{ pet: string; i: number; n: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lit, setLit] = useState<Set<number>>(new Set());
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [tab, setTab] = useState<"transcript" | "note">("note");
  const [editing, setEditing] = useState<engine.Patient | null>(null);
  const audio = useRef<HTMLAudioElement>(null);
  const [audioSrc, setAudioSrc] = useState<string | null>(null);
  const drafted = useRef(false);
  /** Deleting the visit (ADR-0022), and the recording dialog. */
  const [deleting, setDeleting] = useState(false);
  const [askRecording, setAskRecording] = useState(false);
  /** "Show the template": the latest draft's template, read-only. */
  const [shownTemplate, setShownTemplate] = useState<engine.TemplateView | null>(null);

  /** In a multi-pet visit, the animal whose note is shown (M2 item 2). */
  const [pet, setPet] = useState<string | null>(initialPet ?? null);
  const load = useCallback(
    () => engine.openSession(id, pet).then(setView, (e) => setError(String(e?.message ?? e))),
    [id, pet],
  );

  useEffect(() => {
    void load();
    engine.listTemplates().then(setTemplates, () => {});
  }, [load]);

  /** The template a draft is using, for the board's list of headings. */
  const loadDraftTemplate = useCallback(
    (templateId: string) =>
      engine.template(templateId).then(
        (v) => setDraftingWith(v.template),
        () => setDraftingWith(null),
      ),
    [],
  );

  // The recording is decrypted for playback (ADR-0008).
  const viewId = view?.id;
  /** The desktop fallback's blob: URL ("" while it loads), for this visit. */
  const fallback = useRef<string | null>(null);
  const shownId = useRef<string | undefined>(undefined);
  shownId.current = viewId;
  useEffect(() => {
    // A visit whose recording was deleted has nothing to play.
    if (!view || !viewId || view.has_audio === false) return;
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
      if (fallback.current) URL.revokeObjectURL(fallback.current);
      fallback.current = null;
    };
    // Only refetch when the encounter changes, not on every view refresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewId]);

  /** The desktop player couldn't play its URL: try the recording from memory, once. */
  function onAudioError() {
    if (!view || fallback.current !== null || !audioSrc || audioSrc.startsWith("blob:")) return;
    const forId = view.id;
    fallback.current = "";
    engine.audioFallbackUrl(view).then(
      (u) => {
        if (!u) return;
        if (shownId.current !== forId || fallback.current !== "") {
          URL.revokeObjectURL(u);
          return;
        }
        fallback.current = u;
        setAudioSrc(u);
      },
      () => undefined,
    );
  }

  const segments = view?.transcript?.segments ?? [];
  const run = view?.latest_run ?? null;
  // Notes render under the headings they were drafted to (saved with the draft).
  const template = view?.latest_template ?? null;
  // A phone call drafts with the Phone call template unless another is chosen (ADR-0023 section 8).
  const chosen = picked ?? template?.id ?? (view?.call ? "callback" : app.settings.template);

  useEffect(() => {
    if (!template) return;
    engine.template(template.id).then(setCurrent, () => setCurrent(null));
  }, [template]);

  // A multi-pet visit opens on its first animal.
  useEffect(() => {
    if (view && view.pets.length > 1 && !pet) setPet(view.pets[0] ?? null);
  }, [view, pet]);

  const draft = useCallback(async () => {
    setError(null);
    setGenerating(true);
    void loadDraftTemplate(chosen);
    tracker.start();
    try {
      const done = await engine.generateNote(id, app.settings.llm, chosen, app.settings.noteLanguage, pet);
      await load();
      if (done.notice) app.toast(t(done.notice));
      app.announce(t("Draft ready"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      tracker.stop();
      setGenerating(false);
    }
  }, [id, app, load, chosen, t, pet, tracker, loadDraftTemplate]);

  /** After a multi-pet recording: one note per animal, one after another. */
  const draftAll = useCallback(
    async (pets: string[]) => {
      setError(null);
      setGenerating(true);
      void loadDraftTemplate(chosen);
      try {
        for (const [i, p] of pets.entries()) {
          setPetRound({ pet: p, i: i + 1, n: pets.length });
          tracker.start();
          await engine.generateNote(id, app.settings.llm, chosen, app.settings.noteLanguage, p);
        }
        await load();
        app.announce(t("Draft ready"));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        tracker.stop();
        setPetRound(null);
        setGenerating(false);
      }
    },
    [id, app, load, chosen, t, tracker, loadDraftTemplate],
  );

  async function draftDocument(templateId: string, language = docLanguage) {
    setError(null);
    setDrafting(templateId);
    setOpenDoc(templateId);
    void loadDraftTemplate(templateId);
    tracker.start();
    try {
      await engine.generateNote(id, app.settings.llm, templateId, language, pet);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setOpenDoc(null);
    } finally {
      tracker.stop();
      setDrafting(null);
    }
  }

  async function saveEdit(heading: string, value: unknown, dentition?: Dentition | null) {
    if (!template) return;
    if (!app.actorName.trim()) {
      setError(t("Add your name in Settings, then save your edits."));
      return;
    }
    setError(null);
    setSavingEdit(true);
    try {
      await engine.editNote(id, heading, value, app.actor, pet, dentition);
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
    if (!app.actorName.trim()) {
      setError(t("Add your name in Settings, then sign the addendum."));
      return;
    }
    try {
      setView(await engine.addAddendum(id, addendum, app.actor, pet));
      setAddendum(null);
      app.toast(t("Addendum signed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function refine(
    heading: string,
    request: string,
    saveAsStandard: boolean,
    choice?: { scope: engine.Scope; approver: engine.Approver | null },
  ) {
    if (!template) return;
    setError(null);
    setRefining(true);
    tracker.start();
    try {
      await engine.refineNote(id, {
        llm: app.settings.llm,
        heading,
        request,
        save_as_standard: saveAsStandard,
        by: app.actor.trim() || null,
        language: app.settings.noteLanguage,
        pet,
        standard_scope: choice?.scope ?? null,
        approver: choice?.approver ?? null,
      });
      await load();
      setAsking(null);
      const title = engine.headingTitle(template, heading);
      app.toast(
        saveAsStandard
          ? t("{heading} redrafted and saved as a standard", { heading: title })
          : t("{heading} redrafted", { heading: title }),
      );
      app.announce(t("{heading} redrafted", { heading: title }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      tracker.stop();
      setRefining(false);
    }
  }

  useEffect(() => {
    const wanted = draftTemplate != null || (autoDraft && !view?.latest_run);
    if (wanted && view && segments.length > 0 && !drafted.current) {
      if (view.pets.length > 1 && !pet) return; // wait until the first animal is chosen
      drafted.current = true;
      if (view.pets.length > 1 && draftTemplate == null) void draftAll(view.pets);
      else void draft();
    }
  }, [autoDraft, draftTemplate, view, segments.length, draft, draftAll, pet]);

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
    if (pt && `${pt.name} ${pt.last_name}`.trim())
      lines.push(`${t("Patient")}: ${`${pt.name} ${pt.last_name}`.trim()}`);
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
        ? (view?.addenda ?? []).map(
            (a) => [`${t("Addendum")}, ${engine.fmtStamp(a.at)}, ${a.by}`, a.text] as [string, string],
          )
        : [],
      notDiscussed: t("Not discussed"),
      t,
      lowerInner,
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
        pet,
      });
      if (saved) app.toast(t("PDF saved"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  /** A veterinarian confirms (or takes back) drafted teeth against the recording. */
  async function confirmTeeth(teeth: string[], confirmed: boolean) {
    if (!app.actorName.trim()) {
      setError(t("Add your name in Settings, then confirm the chart."));
      return;
    }
    setError(null);
    setConfirming(true);
    try {
      setView(await engine.confirmChart(id, teeth, confirmed, app.actor, pet));
      app.announce(
        confirmed
          ? t("Tooth {tooth} confirmed", { tooth: teeth.join(", ") })
          : t("Tooth {tooth} is a draft again", { tooth: teeth.join(", ") }),
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setConfirming(false);
    }
  }

  async function sign() {
    if (!app.actorName.trim()) {
      setError(t("Add your name in Settings, then sign."));
      return;
    }
    try {
      setView(await engine.signNote(id, app.actor, pet));
      app.toast(t("Note signed"));
      app.announce(t("Note signed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  /** Engine messages without a name in them are translated (the plan's copy table). */
  const errorText = (e: unknown) => t(e instanceof Error ? e.message : String(e));

  /**
   * Stop the player, and any download of the recording still under way (web
   * mode streams it), before the visit's files move. Returns a function that
   * puts the player back if the deletion doesn't happen.
   */
  function stopPlayer(): () => void {
    const a = audio.current;
    const src = a?.getAttribute("src");
    if (!a || !src) return () => {};
    a.pause();
    a.removeAttribute("src");
    a.load();
    return () => {
      a.setAttribute("src", src);
      a.load();
    };
  }

  /** Delete the visit with a reason: to Deleted visits, with Undo in the toast. */
  async function deleteVisit(kind: engine.ReasonKind) {
    if (!view) return;
    if (!app.actorName.trim()) {
      setError(t("Add your name in Settings, then delete the visit."));
      return;
    }
    setError(null);
    setDeleting(true);
    const by = app.actor.trim() || null;
    const shownPet = pet;
    // Back to the page of the animal this visit was opened for.
    const back = patientIdFor(view.patient, shownPet);
    const resumePlayer = stopPlayer();
    try {
      const done = await engine.deleteVisit(id, by, { kind });
      app.toast(t("Visit deleted"), {
        label: t("Undo"),
        run: () => {
          engine.restoreVisit(done.trash_id, by).then(
            (r) => {
              app.toast(t("Visit restored"));
              app.navigate({ screen: "review", id: r.id, pet: shownPet ?? undefined });
            },
            (e) => app.toast(errorText(e)),
          );
        },
      });
      app.announce(t("Visit deleted"));
      app.navigate(back ? { screen: "patient", id: back } : { screen: "patients" });
    } catch (e) {
      resumePlayer();
      setError(errorText(e));
      setDeleting(false);
    }
  }

  /** Delete only the recording; the transcript and notes stay. */
  async function deleteRecording() {
    const resumePlayer = stopPlayer();
    try {
      await engine.deleteRecording(id, app.actor.trim() || null);
    } catch (e) {
      resumePlayer();
      throw new Error(errorText(e));
    }
    if (audioSrc?.startsWith("blob:")) URL.revokeObjectURL(audioSrc);
    fallback.current = null;
    setAudioSrc(null);
    await load();
    app.toast(t("Recording deleted"));
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
  // Tooth chart flags (ADR-0021): a tooth outside the mouth, or a dog-only
  // tooth with no dentition chosen, blocks signing; the rest need a look.
  // Counted once per item, and not again when the item is already counted.
  const teethFlags = toothFlagPaths(run?.faithfulness);
  const otherSoft = new Set([
    ...(run?.faithfulness.citation_mismatches ?? []).map((m) => m.path),
    ...(run?.faithfulness.unsupported_statements ?? []).map((u) => u.path),
  ]);
  const toothBlocks = teethFlags.blocking.size;
  /** Drafted teeth no veterinarian has confirmed: signing waits for them (ADR-0021 decision 10). */
  const unconfirmed = view?.unconfirmed_teeth ?? [];
  const softFlags =
    (run?.faithfulness.citation_mismatches.length ?? 0) +
    (run?.faithfulness.invalid_evidence.length ?? 0) +
    (run?.faithfulness.unverified_values?.length ?? 0) +
    (run?.faithfulness.unsupported_statements?.length ?? 0) +
    [...teethFlags.soft].filter((path) => !otherSoft.has(path)).length;
  const disputed = run?.faithfulness.disputed_numbers?.length ?? 0;
  const signed = Boolean(view?.signed_at);
  /** Any animal's note signed makes the whole visit signed (ADR-0022 section 2). */
  const signedVisit = Boolean(view?.any_signed ?? view?.signed_at);
  const otherPets = view && view.pets.length > 1 ? view.pets.filter((x) => x !== (pet ?? view.pets[0])) : [];
  const importedFile = view?.source && view.source !== "Recorded" ? view.source : null;
  const canPlay = Boolean(view && audioSrc && view.has_audio !== false);
  const lines = segments.length;
  const lastSecs = run ? run.stats.total_ms / 1000 : null;
  const draftingName = draftingWith ? templateName(t, draftingWith.name) : "";
  /** The board for the note: a first draft, a redraft, or one animal of several. */
  const noteBoard = generating && (
    <DraftBoard
      tracker={tracker}
      template={draftingWith}
      title={
        petRound
          ? t("Drafting {pet}'s note", { pet: petRound.pet })
          : run
            ? t("Drafting again with {template}", { template: draftingName })
            : t("Writing note…")
      }
      subtitle={
        petRound
          ? t("Animal {i} of {n}", { i: petRound.i, n: petRound.n })
          : run
            ? t("The current draft stays until the new one is ready. Earlier drafts are kept.")
            : draftingName || undefined
      }
      lines={lines}
      lastSecs={lastSecs}
    />
  );

  return (
    <div className="screen review">
      <header className="review-bar">
        <div className="review-title">
          <h1>{patientName || (view ? view.created_at : t("Visit"))}</h1>
          {view?.stats && <span className="mono faint">{fmtTime(view.stats.audio_secs)}</span>}
        </div>
        <div className="review-actions">
          {signed || templates.length === 0 ? (
            <span className="ob-chip">{template ? templateName(t, template.name) : t("No draft")}</span>
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
                    {templateName(t, x.name)}
                  </option>
                ))}
            </select>
          )}
          {run && (
            <span className="ob-chip model-chip" title={run.model}>
              <span className="ob-dot" data-tone="ok" aria-hidden />
              {`${shortModel(run.model)} · ${inferenceHost(t)}`}
            </span>
          )}
          {signed ? (
            <StatusDot tone="ok">
              {t("Signed by {name}, {at}", { name: view?.signed_by ?? "", at: view?.signed_at ?? "" })}
            </StatusDot>
          ) : run && app.can.sign ? (
            <Button
              variant="primary"
              onClick={sign}
              disabled={generating || refining || confirming || hardFails > 0 || toothBlocks > 0 || unconfirmed.length > 0}
            >
              <Signature size={20} aria-hidden />
              {t("Sign note")}
            </Button>
          ) : run || tab === "note" ? null : (
            // Secondary: the empty note carries the one primary "Draft note" (§7.7). This one
            // shows only on the phone and tablet Transcript tab, where the note pane is hidden
            // (CSS hides it from 1024 px, where both panes show).
            <Button className="draft-head" onClick={draft} disabled={!app.can.notes || generating || segments.length === 0}>
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
            {p?.species && (
              <div>
                <dt className="label muted">{t("Species")}</dt>
                <dd>{t(speciesName(p.species))}</dd>
              </div>
            )}
            <div>
              <dt className="label muted">{t("Recorded")}</dt>
              <dd className="mono">{view?.created_at}</dd>
            </div>
            {view?.call && (
              <div>
                <dt className="label muted">{t("Phone call")}</dt>
                <dd className="mono">{callLine(t, view.call)}</dd>
              </div>
            )}
            <div>
              <dt className="label muted">{t("Consent to record")}</dt>
              <dd>
                {view?.call && view.consent ? (
                  callConsentLine(t, view.call, view.consent)
                ) : view?.consent ? (
                  engine.consentText(view.consent)
                ) : (
                  <span className="faint">{t("Not recorded")}</span>
                )}
              </dd>
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
              {p?.patient_id && <CallButton patientId={p.patient_id} variant="ghost" />}
              {view && app.can.notes && (
                <DeleteVisitMenu
                  others={otherPets.length > 0 ? listNames(app.settings.uiLanguage, otherPets) : undefined}
                  busy={deleting || generating || refining}
                  disabledReason={
                    signedVisit ? t("Signed visits can't be deleted yet. Add an addendum to correct the note.") : null
                  }
                  onDelete={(kind) => void deleteVisit(kind)}
                />
              )}
            </div>
          </dl>
        )}
      </section>

      {(view?.pets.length ?? 0) > 1 && (
        <div className="pet-tabs ob-segmented" role="tablist" aria-label={t("Animals in this visit")}>
          {view?.pets.map((p) => (
            <button
              key={p}
              className="ob-segment"
              role="tab"
              aria-selected={p === pet}
              disabled={generating || refining}
              onClick={() => {
                setAsking(null);
                setEditingHeading(null);
                setPet(p);
              }}
            >
              {p}
            </button>
          ))}
        </div>
      )}

      <div className="review-tabs ob-segmented" role="tablist">
        <button
          className="ob-segment"
          role="tab"
          aria-selected={tab === "transcript"}
          onClick={() => setTab("transcript")}
        >
          {t("Transcript")}
        </button>
        <button className="ob-segment" role="tab" aria-selected={tab === "note"} onClick={() => setTab("note")}>
          {t("Note")}
        </button>
      </div>

      <div className="review-split" data-tab={tab}>
        <section className="pane pane-transcript" aria-label={t("Transcript")}>
          {canPlay && audioSrc && (
            <>
              <audio
                ref={audio}
                className="player"
                controls
                preload="metadata"
                src={audioSrc}
                onError={onAudioError}
                onTimeUpdate={onTime}
                onPause={() => setPlayingId(null)}
              />
              {app.can.notes && (
                <div className="player-actions">
                  <span
                    className="delete-recording"
                    title={
                      signedVisit && !app.can.sign
                        ? t("Only a veterinarian can delete the recording of a signed visit.")
                        : undefined
                    }
                  >
                    <Button
                      variant="ghost"
                      size="compact"
                      disabled={(signedVisit && !app.can.sign) || generating || refining}
                      onClick={() => setAskRecording(true)}
                    >
                      <Trash2 size={16} aria-hidden />
                      {t("Delete recording")}
                    </Button>
                  </span>
                </div>
              )}
            </>
          )}
          {view?.audio_deleted && (
            <p className="recording-gone small muted">
              {t("Recording deleted {date} by {name}. The transcript and notes are kept.", {
                date: engine.fmtStamp(view.audio_deleted.at),
                name: view.audio_deleted.by.name,
              })}
            </p>
          )}
          {segments.length > 0 ? (
            <TranscriptList
              segments={segments}
              highlighted={lit}
              playingId={playingId}
              onSeek={seek}
              speakerNames={view?.call ? [t(view.call.clinic_label), t("Client")] : undefined}
            />
          ) : view?.call && view.consent?.outcome === "declined" ? (
            <p className="pane-pad muted">{t("Not recorded. The client declined.")}</p>
          ) : view?.call && view.consent?.outcome === "not_announced" ? (
            <p className="pane-pad muted">{t("Not recorded. The announcement wasn't confirmed as read.")}</p>
          ) : (
            <div className="pane-pad">
              <Skeleton lines={4} label={t("Loading")} />
            </div>
          )}
        </section>

        <section className="pane pane-note" aria-label={t("Note")}>
          {noteBoard && <div className="draft-board-wrap">{noteBoard}</div>}
          {run && template ? (
            <div className={generating ? "note-under-redraft" : undefined} aria-busy={generating || undefined}>
              {(() => {
                const label = draftLabel(t, template, app.user?.id ?? null, app.status?.templates ?? "alone");
                return (
                  <div className="draft-origin">
                    {label && <span className="small muted">{t("Drafted with {label}", { label })}</span>}
                    <span className="draft-origin-actions">
                      <Button
                        variant="ghost"
                        size="compact"
                        onClick={() => app.navigate({ screen: "templates", open: template.id })}
                      >
                        {t("Edit template")}
                      </Button>
                      <Button
                        variant="ghost"
                        size="compact"
                        onClick={() => engine.runTemplateView(id, pet).then(setShownTemplate, (e) => setError(String(e?.message ?? e)))}
                      >
                        {t("Show the template")}
                      </Button>
                    </span>
                  </div>
                );
              })()}
              {(hardFails > 0 || toothBlocks > 0 || unconfirmed.length > 0 || softFlags > 0 || disputed > 0) && (
                <p className="ob-notice note-summary" data-tone={hardFails || toothBlocks ? "danger" : undefined}>
                  {hardFails > 0 &&
                    `${hardFails === 1 ? t("1 number isn't in the transcript.") : t("{n} numbers aren't in the transcript.", { n: hardFails })} `}
                  {toothBlocks > 0 &&
                    `${toothBlocks === 1 ? t("1 tooth on the chart blocks signing until it's corrected.") : t("{n} teeth on the chart block signing until they're corrected.", { n: toothBlocks })} `}
                  {unconfirmed.length > 0 &&
                    `${unconfirmed.length === 1 ? t("1 drafted tooth on the chart isn't confirmed yet. Signing waits until a veterinarian confirms it.") : t("{n} drafted teeth on the chart aren't confirmed yet. Signing waits until a veterinarian confirms each one.", { n: unconfirmed.length })} `}
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
                  signed || generating || !app.can.notes
                    ? undefined
                    : (heading) => {
                        setEditingHeading(null);
                        setAsking((a) => (a === heading ? null : heading));
                      }
                }
                onEdit={
                  signed || generating || refining || !app.can.notes
                    ? undefined
                    : (heading, opts) => {
                        setAsking(null);
                        setEditingTeeth(opts?.tooth ? [opts.tooth] : []);
                        // A tooth click opens the chart editor; the Edit button toggles it.
                        setEditingHeading((h) => (h === heading && !opts?.tooth ? null : heading));
                      }
                }
                suggestedDentition={!pet || pet === view?.pets[0] ? dentitionOf(p?.species) : null}
                chartChecks={view?.chart_checks ?? []}
                unconfirmedTeeth={signed ? [] : unconfirmed}
                onConfirmTeeth={signed || generating || refining || !app.can.sign ? undefined : (teeth, ok) => void confirmTeeth(teeth, ok)}
                confirming={confirming}
                onChooseDentition={(heading, dentition) => {
                  const [sec, part] = heading.split("/");
                  const sectionValue = run.note[sec ?? ""] as Record<string, unknown> | undefined;
                  void saveEdit(heading, asItems(part ? sectionValue?.[part] : sectionValue), dentition);
                }}
                renderEdit={(heading) => {
                  if (editingHeading !== heading) return null;
                  const def = headingDef(template, heading);
                  if (!def) return null;
                  const [sec, part] = heading.split("/");
                  const sectionValue = run.note[sec ?? ""] as Record<string, unknown> | undefined;
                  if (def.kind === "teeth")
                    return (
                      <DentalChartEditor
                        key={editingTeeth.join(",")}
                        title={engine.headingTitle(template, heading)}
                        dentition={def.dentition ?? null}
                        value={part ? sectionValue?.[part] : sectionValue}
                        busy={savingEdit}
                        initialSelection={editingTeeth}
                        onSave={(items, dentition) => void saveEdit(heading, items, dentition)}
                        onCancel={() => setEditingHeading(null)}
                        onPlay={playIds}
                        onEvidence={(ids) => setLit(new Set(ids ?? []))}
                        lineText={(n) => segments.find((s) => s.id === n)?.text}
                      />
                    );
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
                      templateName={templateName(t, template.name)}
                      examples={headingExamples(current?.template ?? template, heading)}
                      standards={(current?.template.standards ?? []).filter((s) => s.heading === heading)}
                      canSave={current != null && (current.can.standards_personal || current.mode !== "personal" || current.can.standards_team_needs_pin)}
                      scopes={
                        (app.status?.templates ?? "alone") === "alone"
                          ? undefined
                          : {
                              personal: app.status?.templates === "personal",
                              teamNeedsPin: true,
                              me: app.user,
                            }
                      }
                      onEditHeading={() => app.navigate({ screen: "templates", open: template.id, heading })}
                      busy={refining}
                      board={
                        <DraftBoard
                          compact
                          tracker={tracker}
                          template={null}
                          title={t("Redrafting {heading}", { heading: engine.headingTitle(template, heading) })}
                          lines={lines}
                        />
                      }
                      onSubmit={(request, save, choice) => void refine(heading, request, save, choice)}
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
                  <FileDown size={20} aria-hidden />
                  {t("Save PDF")}
                </Button>
                {!signed && app.can.notes && (
                  <Button variant="ghost" onClick={draft} disabled={generating || refining}>
                    <RotateCw size={20} aria-hidden />
                    {chosen === template?.id
                      ? t("Draft again")
                      : t("Draft as {name}", {
                          name: templateName(t, templates.find((x) => x.id === chosen)?.name ?? chosen),
                        })}
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
                        {templateName(t, x.name)}
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
                  {run.stats.total_ms > 0 &&
                    ` ${t("Drafted in {s} s.", { s: (run.stats.total_ms / 1000).toFixed(1) })}`}
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
                  {!app.can.sign ? null : addendum === null ? (
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
                      <p className="small faint">
                        {t("The signed note doesn't change. The addendum is dated and signed with your name.")}
                      </p>
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
            </div>
          ) : (
            !generating && (
              <div className="pane-pad empty-state">
                <MarkMono size={48} className="faint" />
                <h3>{t("No draft yet")}</h3>
                <p className="muted">{t("Each line links back to the moment it was said.")}</p>
                <Button variant="primary" onClick={draft} disabled={!app.can.notes || segments.length === 0}>
                  {t("Draft note")}
                </Button>
              </div>
            )
          )}
        </section>
      </div>
      {shownTemplate && (
        <Sheet label={t("The template this draft was written with")} onClose={() => setShownTemplate(null)}>
          {(close) => (
            <>
              <header className="sheet-head">
                <h2>{draftLabel(t, shownTemplate.template, app.user?.id ?? null, app.status?.templates ?? "alone") ?? templateName(t, shownTemplate.template.name)}</h2>
                <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
                  <X size={20} aria-hidden />
                </button>
              </header>
              <div className="sheet-body">
                <p className="small muted">
                  {t("Exactly what the model was told when this draft was written. Changing the template later doesn't change this note.")}
                </p>
                <pre className="prompt">{shownTemplate.prompt}</pre>
              </div>
            </>
          )}
        </Sheet>
      )}
      {askRecording && view && (
        <DeleteRecordingDialog
          signed={signedVisit}
          multiPet={view.pets.length > 1}
          importedFile={importedFile}
          onConfirm={deleteRecording}
          onClose={() => setAskRecording(false)}
        />
      )}
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
          board={
            <DraftBoard
              tracker={tracker}
              template={draftingWith}
              title={t("Drafting {name}", {
                name: templateName(t, templates.find((x) => x.id === openDoc)?.name ?? openDoc),
              })}
              subtitle={t("Written from your note, with your corrections, in plain language.")}
              lines={lines}
            />
          }
          onCopy={(doc) =>
            doc.template && doc.run ? void copyNote(doc.template, doc.run, doc.name, false) : undefined
          }
          onPdf={() => void savePdf(openDoc)}
          onRedraft={() => void draftDocument(openDoc)}
          language={docLanguage}
          onLanguage={(code) => {
            setDocLanguage(code);
            void draftDocument(openDoc, code);
          }}
          onClose={() => setOpenDoc(null)}
          onEvidence={(ids) => setLit(new Set(ids ?? []))}
          onPlay={playIds}
        />
      )}
    </div>
  );
}

/** What goes under a heading path, from the draft's template. */
function headingDef(
  t: engine.Template,
  heading: string,
): { kind: engine.ContentKind; fields?: engine.TemplateField[]; dentition?: Dentition | null } | null {
  const [sec, part] = heading.split("/");
  const s = t.sections.find((x) => x.id === sec);
  if (!s) return null;
  if (!part) return s.kind ? { kind: s.kind, fields: s.fields, dentition: s.dentition } : null;
  const p = s.parts?.find((x) => x.key === part);
  return p ? { kind: p.kind, fields: p.fields, dentition: p.dentition } : null;
}

/** "gemma-4-E4B_q4_0-it" reads "gemma-4-e4b" on the chip; the full id is its tooltip. */
function shortModel(id: string): string {
  return id
    .toLowerCase()
    .replace(/\.gguf$/, "")
    .replace(/[-_]it(?=[-_.]|$)/, "")
    .replace(/[-_]q\d\w*$/, "");
}

/**
 * Where the note was written (docs/brand/DESIGN.md §7.6: always show where
 * inference runs). On this computer it's "local"; a browser reaching the
 * clinic's OpenBark server names that machine.
 */
function inferenceHost(t: ReturnType<typeof useT>): string {
  const host = typeof window === "undefined" ? "" : window.location.hostname;
  return engine.isDesktop || !host || host === "localhost" || host === "127.0.0.1" || host === "[::1]"
    ? t("local")
    : host;
}

type Translate = (text: string, vars?: Record<string, string | number>) => string;

/** "Outgoing · 4:12 · +1 555 555 0142" (ADR-0023 section 8). */
function callLine(t: Translate, call: engine.CallInfo): string {
  return [t(call.direction === "outgoing" ? "Outgoing" : "Incoming"), callLength(call.duration_secs), formatPhone(call.number)].join(" · ");
}

/** The announcement and the client's answer, as Review shows them. */
function callConsentLine(t: Translate, call: engine.CallInfo, c: engine.RecordingConsent): string {
  if (c.outcome === "not_announced") {
    return t("The announcement wasn't confirmed as read, so the recording was deleted at {time}.", {
      time: callLength(call.declined_at_secs ?? 0),
    });
  }
  if (c.outcome === "declined") {
    return t("Client declined recording at {time}. The recording was deleted.", { time: callLength(call.declined_at_secs ?? 0) });
  }
  const at = callLength(c.announced_at_secs ?? 0);
  const read =
    c.method === "announced"
      ? t("Announcement played at {time}.", { time: at })
      : t("Announcement read by {name} at {time}.", { name: c.confirmed_by, time: at });
  return `${read} ${c.outcome === "agreed" ? t("Client agreed.") : t("Client stayed on the line.")}`;
}
