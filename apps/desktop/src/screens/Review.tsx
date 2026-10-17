import { Button, StatusDot } from "@openbark/ui";
import { ArrowClockwise, Copy, FolderOpen, Signature } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { NoteView } from "../components/NoteView";
import { TranscriptList } from "../components/TranscriptList";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";

/** DESIGN.md §6 Review: slim top bar, 2fr/3fr transcript | note, output row. */
export function Review({ id, autoDraft }: { id: string; autoDraft: boolean }) {
  const app = useApp();
  const [view, setView] = useState<engine.SessionView | null>(null);
  const [template, setTemplate] = useState<engine.Template | null>(null);
  const [generating, setGenerating] = useState(false);
  const [tokens, setTokens] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [lit, setLit] = useState<Set<number>>(new Set());
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [tab, setTab] = useState<"transcript" | "note">("note");
  const audio = useRef<HTMLAudioElement>(null);
  const drafted = useRef(false);

  const load = useCallback(() => engine.openSession(id).then(setView, (e) => setError(String(e?.message ?? e))), [id]);

  useEffect(() => {
    void load();
    engine.template("soap").then(setTemplate, () => {});
    const off = engine.on("token", () => setTokens((n) => n + 1));
    return () => void off.then((f) => f());
  }, [load]);

  const segments = view?.transcript?.segments ?? [];
  const run = view?.latest_run ?? null;

  const draft = useCallback(async () => {
    setError(null);
    setTokens(0);
    setGenerating(true);
    try {
      await engine.generateNote(id, app.settings.llm, "soap");
      await load();
      app.announce("Draft ready");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setGenerating(false);
    }
  }, [id, app, load]);

  useEffect(() => {
    if (autoDraft && view && !view.latest_run && segments.length > 0 && !drafted.current) {
      drafted.current = true;
      void draft();
    }
  }, [autoDraft, view, segments.length, draft]);

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

  const hardFails = run?.faithfulness.unsupported_numbers.length ?? 0;
  const softFlags = (run?.faithfulness.citation_mismatches.length ?? 0) + (run?.faithfulness.invalid_evidence.length ?? 0);
  const signed = Boolean(view?.signed_at);
  const progress = Math.min(0.95, tokens / 1400);

  return (
    <div className="screen review">
      <header className="review-bar">
        <div className="review-title">
          <h1>{view ? view.created_at : "Encounter"}</h1>
          <span className="small muted">{view?.source}</span>
          {view?.stats && <span className="mono faint">{fmtTime(view.stats.audio_secs)}</span>}
        </div>
        <div className="review-actions">
          <span className="ob-chip">SOAP note</span>
          {run && <span className="ob-chip mono">{run.model}</span>}
          {signed ? (
            <StatusDot tone="ok">
              Signed by {view?.signed_by}, {view?.signed_at}
            </StatusDot>
          ) : run ? (
            <Button variant="primary" onClick={sign} disabled={generating || hardFails > 0}>
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
              {(hardFails > 0 || softFlags > 0) && (
                <p className="ob-notice note-summary" data-tone={hardFails ? "danger" : undefined}>
                  {hardFails > 0 && `${hardFails} number${hardFails === 1 ? " isn't" : "s aren't"} in the transcript. `}
                  {softFlags > 0 && `${softFlags} item${softFlags === 1 ? "" : "s"} cite the wrong transcript line. `}
                  Rest on a sentence to see its evidence.
                </p>
              )}
              <NoteView
                template={template}
                run={run}
                onEvidence={(ids) => setLit(new Set(ids ?? []))}
                onPlay={playIds}
                onCopySection={(title, text) => copy(`${title}\n${text}`, title)}
              />
              <footer className="output-row">
                <Button onClick={() => copy(view?.latest_markdown ?? "", "Note")}>
                  <Copy size={20} aria-hidden />
                  Copy note
                </Button>
                {!signed && (
                  <Button variant="ghost" onClick={draft} disabled={generating}>
                    <ArrowClockwise size={20} aria-hidden />
                    Draft again
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
                  against the transcript. Drafted in {(run.stats.total_ms / 1000).toFixed(1)} s.
                </span>
              </footer>
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
