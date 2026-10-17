// One engine interface, two transports:
// - desktop (Tauri): commands in src-tauri/src/lib.rs, events via Tauri
// - web (`openbark serve`): HTTP API in engine/os-server, events via SSE
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

export interface Segment {
  id: number;
  start: number;
  end: number;
  speaker: number | null;
  text: string;
}

export interface Transcript {
  segments: Segment[];
}

export interface TranscribeStats {
  audio_secs: number;
  total_ms: number;
  rtf: number;
  segments: number;
  speakers: number;
}

export interface NoteStats {
  prompt_tokens: number;
  completion_tokens: number;
  time_to_first_token_ms: number;
  total_ms: number;
  tokens_per_second: number;
}

export interface Faithfulness {
  passed: boolean;
  unsupported_numbers: { path: string; number: string; cited: number[] }[];
  citation_mismatches: { path: string; number: string; cited: number[] }[];
  invalid_evidence: string[];
}

export interface SessionSummary {
  id: string;
  dir: string;
  created_at: string;
  source: string;
  audio_secs: number;
  runs: number;
}

export interface SessionView {
  id: string;
  dir: string;
  /** Desktop: a local file path. */
  audio_path?: string;
  /** Web: an API URL. */
  audio_url?: string;
  transcript: Transcript | null;
  stats: TranscribeStats | null;
  latest_note: string | null;
}

export interface NoteResponse {
  markdown: string;
  run_dir: string;
  result: {
    status: "draft";
    template: string;
    model: string;
    note: unknown;
    faithfulness: Faithfulness;
    stats: NoteStats;
  };
}

export interface Status {
  product: string;
  version: string;
  mode?: "web";
  models_dir: string;
  sessions_dir: string;
  models: { id: string; name: string; installed: boolean }[];
  llm_backend: string;
}

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `${res.status} ${res.statusText}`);
  return body as T;
}

export const status = () => (isDesktop ? invoke<Status>("status") : api<Status>("/api/status"));

export const listSessions = () =>
  isDesktop ? invoke<SessionSummary[]>("list_sessions") : api<SessionSummary[]>("/api/sessions");

export const openSession = (id: string) =>
  isDesktop
    ? invoke<SessionView>("open_session", { dir: id })
    : api<SessionView>(`/api/sessions/${encodeURIComponent(id)}`);

function pickBrowserFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".wav,audio/wav";
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

/**
 * Ask the user for a recording, then create a saved session and transcribe it.
 * `onPicked` fires once a file is chosen (before the long transcription).
 * Resolves to null if the user cancels.
 */
export async function newSessionFromFile(diarize: boolean, onPicked: () => void): Promise<SessionView | null> {
  if (isDesktop) {
    const picked = await open({ multiple: false, filters: [{ name: "Recording (WAV)", extensions: ["wav"] }] });
    if (typeof picked !== "string") return null;
    onPicked();
    return invoke<SessionView>("transcribe", { path: picked, diarize });
  }
  const file = await pickBrowserFile();
  if (!file) return null;
  onPicked();
  const q = new URLSearchParams({ diarize: String(diarize), name: file.name });
  return api<SessionView>(`/api/sessions?${q}`, {
    method: "POST",
    headers: { "content-type": "audio/wav" },
    body: file,
  });
}

export const generateNote = (sessionId: string, llm: string, template: string) =>
  isDesktop
    ? invoke<NoteResponse>("generate_note", { sessionDir: sessionId, llm, template })
    : api<NoteResponse>(`/api/sessions/${encodeURIComponent(sessionId)}/notes`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ llm, template }),
      });

export const audioSrc = (s: SessionView) =>
  isDesktop && s.audio_path ? convertFileSrc(s.audio_path) : (s.audio_url ?? "");

/** Show a folder in the file manager (desktop only). */
export const canReveal = isDesktop;
export const reveal = (path: string) => revealItemInDir(path);

type Unlisten = () => void;
let source: EventSource | null = null;
const handlers = { segment: new Set<(s: Segment) => void>(), token: new Set<(t: string) => void>() };

function sse() {
  if (source) return;
  source = new EventSource("/api/events");
  source.onmessage = (e) => {
    const msg = JSON.parse(e.data) as { type: "segment"; data: Segment } | { type: "token"; data: string };
    if (msg.type === "segment") handlers.segment.forEach((h) => h(msg.data));
    else handlers.token.forEach((h) => h(msg.data));
  };
}

export async function onSegment(cb: (s: Segment) => void): Promise<Unlisten> {
  if (isDesktop) return listen<Segment>("transcript-segment", (e) => cb(e.payload));
  sse();
  handlers.segment.add(cb);
  return () => handlers.segment.delete(cb);
}

export async function onNoteToken(cb: (piece: string) => void): Promise<Unlisten> {
  if (isDesktop) return listen<string>("note-token", (e) => cb(e.payload));
  sse();
  handlers.token.add(cb);
  return () => handlers.token.delete(cb);
}
