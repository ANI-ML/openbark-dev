import { Button, ThemeToggle } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import * as engine from "./engine";
import { NoteMarkdown } from "./markdown";

type Phase = "idle" | "transcribing" | "transcribed" | "generating" | "done";

const LLMS = [
  { id: "gemma-4-e4b-it-q4_0", label: "Gemma 4 E4B (fast)" },
  { id: "gemma-4-12b-it-q4_0", label: "Gemma 4 12B (quality)" },
];

const fmtTime = (s: number) =>
  `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

export function App() {
  const [status, setStatus] = useState<engine.Status | null>(null);
  const [sessions, setSessions] = useState<engine.SessionSummary[]>([]);
  const [session, setSession] = useState<engine.SessionView | null>(null);
  const [diarize, setDiarize] = useState(true);
  const [llm, setLlm] = useState(LLMS[0]!.id);
  const [phase, setPhase] = useState<Phase>("idle");
  const [segments, setSegments] = useState<engine.Segment[]>([]);
  const [tokens, setTokens] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const [noteStats, setNoteStats] = useState<engine.NoteResponse["result"] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [playingId, setPlayingId] = useState<number | null>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const transcriptEnd = useRef<HTMLLIElement>(null);

  const refreshSessions = () => engine.listSessions().then(setSessions, () => {});

  useEffect(() => {
    engine.status().then(setStatus, (e) => setError(String(e)));
    refreshSessions();
    const unlisten = [
      engine.onSegment((s) => setSegments((prev) => [...prev, s])),
      engine.onNoteToken(() => setTokens((n) => n + 1)),
    ];
    return () => unlisten.forEach((p) => p.then((f) => f()));
  }, []);

  useEffect(() => {
    if (phase === "transcribing") transcriptEnd.current?.scrollIntoView({ block: "nearest" });
  }, [segments.length, phase]);

  const missing = status?.models.filter((m) => !m.installed) ?? [];
  const busy = phase === "transcribing" || phase === "generating";

  function showSession(view: engine.SessionView) {
    setSession(view);
    setSegments(view.transcript?.segments ?? []);
    setNote(view.latest_note);
    setNoteStats(null);
    setPhase(view.latest_note ? "done" : view.transcript ? "transcribed" : "idle");
  }

  async function newFromFile() {
    setError(null);
    try {
      const view = await engine.newSessionFromFile(diarize, () => {
        setSession(null);
        setSegments([]);
        setNote(null);
        setPhase("transcribing");
      });
      if (!view) return;
      showSession(view);
      refreshSessions();
    } catch (e) {
      setError(String(e));
      setPhase("idle");
    }
  }

  async function reopen(dir: string) {
    if (!dir) return;
    setError(null);
    try {
      showSession(await engine.openSession(dir));
    } catch (e) {
      setError(String(e));
    }
  }

  async function runNote() {
    if (!session) return;
    setError(null);
    setTokens(0);
    setPhase("generating");
    try {
      const res = await engine.generateNote(session.id, llm, "soap");
      setNote(res.markdown);
      setNoteStats(res.result);
      setPhase("done");
      refreshSessions();
    } catch (e) {
      setError(String(e));
      setPhase("transcribed");
    }
  }

  function playFrom(seg: engine.Segment) {
    const audio = audioRef.current;
    if (!audio) return;
    audio.currentTime = seg.start;
    void audio.play();
    setPlayingId(seg.id);
  }

  function onTimeUpdate() {
    const t = audioRef.current?.currentTime ?? 0;
    const current = segments.find((s) => t >= s.start && t < s.end + 0.3);
    setPlayingId(current ? current.id : null);
  }

  async function copy() {
    if (!note) return;
    await navigator.clipboard.writeText(note);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo" aria-hidden>
            ●
          </span>
          OpenBark <span className="badge">pre-alpha</span>
        </div>
        <div className="topbar-right">
          {status && (
            <span className="muted small">
              {status.mode === "web" ? "Web mode (this computer only) · " : ""}
              {status.llm_backend}
            </span>
          )}
          <ThemeToggle />
        </div>
      </header>

      {missing.length > 0 && (
        <div className="ob-notice banner">
          Missing models: {missing.map((m) => m.name).join(", ")}. Run <code>openbark models pull &lt;id&gt;</code>{" "}
          (in-app model download arrives in M1).
        </div>
      )}
      {error && (
        <div className="ob-notice banner error" role="alert">
          {error}
        </div>
      )}

      <section className="controls ob-card">
        <Button variant="primary" onClick={newFromFile} disabled={busy || missing.length > 0}>
          {phase === "transcribing" ? "Transcribing…" : "New from recording…"}
        </Button>
        <label className="check">
          <input type="checkbox" checked={diarize} onChange={(e) => setDiarize(e.target.checked)} disabled={busy} />
          Label speakers
        </label>
        <select
          className="sessions"
          value={session?.id ?? ""}
          onChange={(e) => reopen(e.target.value)}
          disabled={busy}
          aria-label="Recent sessions"
        >
          <option value="">Recent sessions…</option>
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.created_at} · {s.source} · {fmtTime(s.audio_secs)} · {s.runs} note{s.runs === 1 ? "" : "s"}
            </option>
          ))}
        </select>
        {engine.canReveal && (
          <Button
            variant="ghost"
            onClick={() => engine.reveal(session ? session.dir : (status?.sessions_dir ?? ""))}
            disabled={!session && !status}
            title="Show the saved recording, transcript and notes in your file manager"
          >
            Open folder
          </Button>
        )}
      </section>

      {session && (
        <section className="player ob-card" aria-label="Recording">
          <audio
            ref={audioRef}
            controls
            preload="metadata"
            src={engine.audioSrc(session)}
            onTimeUpdate={onTimeUpdate}
            onPause={() => setPlayingId(null)}
          />
          <span className="muted small path" title={session.dir}>
            Saved in {session.dir}
          </span>
        </section>
      )}

      <main className="panes">
        <section className="pane ob-card" aria-label="Transcript">
          <div className="pane-head">
            <h2>Transcript</h2>
            {session?.stats && (
              <span className="muted small">
                {fmtTime(session.stats.audio_secs)} audio · {(session.stats.total_ms / 1000).toFixed(1)} s ·{" "}
                {session.stats.speakers} speaker{session.stats.speakers === 1 ? "" : "s"}
              </span>
            )}
          </div>
          {segments.length === 0 ? (
            <p className="empty">
              {phase === "transcribing"
                ? "Listening…"
                : "Start from a recording, or reopen a saved session. Click any line to hear it."}
            </p>
          ) : (
            <ol className="segments">
              {segments.map((s) => (
                <li
                  key={s.id}
                  data-speaker={s.speaker ?? "x"}
                  data-playing={playingId === s.id || undefined}
                  onClick={() => playFrom(s)}
                  onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), playFrom(s))}
                  tabIndex={0}
                  role="button"
                  aria-label={`Play from ${fmtTime(s.start)}: ${s.text}`}
                  title="Play from here"
                >
                  <span className="seg-meta">
                    <span className="seg-id">[{s.id}]</span>{" "}
                    {s.speaker == null ? "Speaker" : `Speaker ${s.speaker + 1}`} · {fmtTime(s.start)}
                  </span>
                  <span className="seg-text">{s.text}</span>
                </li>
              ))}
              <li ref={transcriptEnd} aria-hidden className="sentinel" />
            </ol>
          )}
        </section>

        <section className="pane ob-card" aria-label="Note">
          <div className="pane-head">
            <h2>SOAP note</h2>
            <div className="pane-actions">
              <select value={llm} onChange={(e) => setLlm(e.target.value)} disabled={busy} aria-label="Model">
                {LLMS.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </select>
              <Button variant="primary" onClick={runNote} disabled={!session?.transcript || busy}>
                {phase === "generating" ? `Writing… ${tokens}` : note ? "Draft again" : "Generate note"}
              </Button>
              <Button onClick={copy} disabled={!note}>
                {copied ? "Copied" : "Copy"}
              </Button>
            </div>
          </div>
          {note ? (
            <>
              <NoteMarkdown source={note} />
              {noteStats && (
                <p className="muted small stats">
                  {noteStats.model} · {noteStats.stats.prompt_tokens} prompt tokens · first token{" "}
                  {(noteStats.stats.time_to_first_token_ms / 1000).toFixed(1)} s ·{" "}
                  {noteStats.stats.tokens_per_second.toFixed(0)} tok/s · total{" "}
                  {(noteStats.stats.total_ms / 1000).toFixed(1)} s
                </p>
              )}
            </>
          ) : (
            <p className="empty">
              {phase === "generating"
                ? "Writing the draft. Numbers are checked against the transcript when it finishes."
                : "The draft note appears here. It stays a draft until a veterinarian reviews it."}
            </p>
          )}
        </section>
      </main>
    </div>
  );
}
