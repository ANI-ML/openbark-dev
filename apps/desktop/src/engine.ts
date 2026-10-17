// One engine interface over two transports (identical behaviour, os-app):
// - desktop (Tauri): commands in src-tauri/src/lib.rs, events via Tauri
// - web (`openbark serve`): HTTP API in engine/os-server, events via SSE,
//   and the browser's microphone streamed over a WebSocket
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

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
  invalid_evidence: string[];
}

export interface NoteRun {
  status: "draft";
  template: string;
  model: string;
  note: Record<string, unknown>;
  faithfulness: Faithfulness;
  stats: { prompt_tokens: number; completion_tokens: number; total_ms: number; tokens_per_second: number; time_to_first_token_ms: number };
}

/** Clinic-entered details, time-stamped by the engine. */
export interface Patient {
  name: string;
  last_name: string;
  reason: string;
  entered_at?: string | null;
  updated_at?: string | null;
}

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
}

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
  models: { id: string; name: string; installed: boolean }[];
  hardware: Hardware;
  recording: boolean;
}

export type Fit = "fits" | "tight" | "wont_fit";
export type Slot = "llm" | "stt" | "vad" | "diarization" | "speaker_embedding";

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
}

export interface HubRepoView {
  repo: { model: HubModel; revision: string };
  files: { path: string; size_bytes: number; fit: Fit }[];
}

export interface TemplateField {
  key: string;
  label: string;
}

export interface TemplatePart {
  key: string;
  label: string;
  kind: "statements" | "fields" | "medications";
  presence: string;
  fields: TemplateField[];
}

export interface TemplateSection {
  id: string;
  title: string;
  kind: "statements" | "fields" | "medications" | null;
  fields: TemplateField[];
  parts: TemplatePart[];
}

export interface Template {
  id: string;
  version: string;
  name: string;
  sections: TemplateSection[];
}

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `${res.status} ${res.statusText}`);
  return body as T;
}

const post = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

const enc = encodeURIComponent;

export const status = () => (isDesktop ? invoke<Status>("status") : api<Status>("/api/status"));
export const inputDevices = () =>
  isDesktop ? invoke<string[]>("input_devices") : api<string[]>("/api/devices");
export const listSessions = () =>
  isDesktop ? invoke<SessionSummary[]>("list_sessions") : api<SessionSummary[]>("/api/sessions");
export const openSession = (id: string) =>
  isDesktop ? invoke<SessionView>("open_session", { id }) : api<SessionView>(`/api/sessions/${enc(id)}`);
export const template = (id: string) =>
  isDesktop ? invoke<Template>("template", { id }) : api<Template>(`/api/templates/${enc(id)}`);
export const generateNote = (id: string, llm: string, tpl: string) =>
  isDesktop
    ? invoke<{ markdown: string; run: NoteRun }>("generate_note", { id, llm, template: tpl })
    : api<{ markdown: string; run: NoteRun }>(`/api/sessions/${enc(id)}/notes`, post({ llm, template: tpl }));
export const updatePatient = (id: string, patient: Patient) =>
  isDesktop
    ? invoke<SessionView>("update_patient", { id, patient })
    : api<SessionView>(`/api/sessions/${enc(id)}/patient`, post(patient));
export const signNote = (id: string, by: string) =>
  isDesktop ? invoke<SessionView>("sign_note", { id, by }) : api<SessionView>(`/api/sessions/${enc(id)}/sign`, post({ by }));
export const models = () => (isDesktop ? invoke<ModelRow[]>("models") : api<ModelRow[]>("/api/models"));
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

export const audioSrc = (s: SessionView) =>
  isDesktop ? convertFileSrc(s.audio_path) : `/api/sessions/${enc(s.id)}/audio`;

export const canReveal = isDesktop;
export const reveal = (path: string) => revealItemInDir(path);

// ---- importing a recording file -------------------------------------------

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

/** Ask for a WAV, then create an encounter from it. Null if cancelled. */
export async function importRecording(
  speakers: boolean,
  patient: Patient | null,
  onPicked: () => void,
): Promise<SessionView | null> {
  const p = hasPatient(patient) ? patient : null;
  if (isDesktop) {
    const picked = await open({ multiple: false, filters: [{ name: "Recording (WAV)", extensions: ["wav"] }] });
    if (typeof picked !== "string") return null;
    onPicked();
    return invoke<SessionView>("import_file", { path: picked, speakers, patient: p });
  }
  const file = await pickBrowserFile();
  if (!file) return null;
  onPicked();
  const q = new URLSearchParams({
    diarize: String(speakers),
    name: file.name,
    patient_name: p?.name ?? "",
    last_name: p?.last_name ?? "",
    reason: p?.reason ?? "",
  });
  return api<SessionView>(`/api/sessions?${q}`, { method: "POST", headers: { "content-type": "audio/wav" }, body: file });
}

// ---- live recording ---------------------------------------------------------

export interface Recording {
  /** Resolves to the saved encounter once the transcript is complete. */
  stop(): Promise<SessionView>;
}

/** Desktop: the engine records the microphone natively (levels arrive as events). */
async function startDesktopRecording(speakers: boolean, device: string | null, patient: Patient | null): Promise<Recording> {
  await invoke<string>("start_recording", { device, speakers, patient });
  return { stop: () => invoke<SessionView>("stop_recording") };
}

const WORKLET = `
class Tap extends AudioWorkletProcessor {
  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (ch) this.port.postMessage(ch.slice(0));
    return true;
  }
}
registerProcessor("openbark-tap", Tap);`;

/**
 * Web: the browser captures the user's own microphone and streams PCM to the
 * engine over a WebSocket. Levels are measured here so the meter reacts to
 * the real input with no round trip.
 */
async function startBrowserRecording(
  speakers: boolean,
  patient: Patient | null,
  onLevel: (l: number, s: number) => void,
): Promise<Recording> {
  if (!navigator.mediaDevices?.getUserMedia) {
    throw new Error("This browser can't record audio. Use a current version of Chrome, Edge, Firefox or Safari.");
  }
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: true },
    });
  } catch {
    throw new Error("Microphone access was blocked. Allow the microphone for this page in the browser's address bar, then try again.");
  }
  const ctx = new AudioContext();
  const url = URL.createObjectURL(new Blob([WORKLET], { type: "application/javascript" }));
  await ctx.audioWorklet.addModule(url);
  URL.revokeObjectURL(url);
  const source = ctx.createMediaStreamSource(stream);
  const tap = new AudioWorkletNode(ctx, "openbark-tap");
  source.connect(tap);

  const proto = location.protocol === "https:" ? "wss" : "ws";
  const q = new URLSearchParams({
    speakers: String(speakers),
    sample_rate: String(ctx.sampleRate),
    patient_name: patient?.name ?? "",
    last_name: patient?.last_name ?? "",
    reason: patient?.reason ?? "",
  });
  const ws = new WebSocket(`${proto}://${location.host}/api/live?${q}`);
  ws.binaryType = "arraybuffer";
  const opened = new Promise<void>((resolve, reject) => {
    ws.onmessage = (e) => {
      const msg = JSON.parse(String(e.data)) as { started?: string; error?: string };
      if (msg.error) reject(new Error(msg.error));
      else resolve();
    };
    ws.onerror = () => reject(new Error("Couldn't reach the OpenBark engine. Check that `openbark serve` is running."));
  });

  let samples = 0;
  let acc = 0;
  let n = 0;
  const every = Math.round(ctx.sampleRate / 20);
  // The worklet posts a copied (non-shared) buffer.
  tap.port.onmessage = (e: MessageEvent<Float32Array<ArrayBuffer>>) => {
    const block = e.data;
    if (ws.readyState === WebSocket.OPEN) ws.send(block);
    samples += block.length;
    for (const v of block) {
      acc += v * v;
      if (++n >= every) {
        const db = 20 * Math.log10(Math.max(1e-6, Math.sqrt(acc / n)));
        onLevel(Math.min(1, Math.max(0, (db + 60) / 60)), samples / ctx.sampleRate);
        acc = 0;
        n = 0;
      }
    }
  };
  try {
    await opened;
  } catch (e) {
    stream.getTracks().forEach((t) => t.stop());
    await ctx.close();
    throw e;
  }

  return {
    stop: () =>
      new Promise<SessionView>((resolve, reject) => {
        tap.port.onmessage = null;
        stream.getTracks().forEach((t) => t.stop());
        void ctx.close();
        ws.onmessage = (e) => {
          const msg = JSON.parse(String(e.data)) as { session?: SessionView; error?: string };
          if (msg.session) resolve(msg.session);
          else reject(new Error(msg.error ?? "The recording couldn't be saved."));
        };
        ws.send("stop");
      }),
  };
}

export function startRecording(
  speakers: boolean,
  device: string | null,
  patient: Patient | null,
  onLevel: (level: number, seconds: number) => void,
): Promise<Recording> {
  const p = hasPatient(patient) ? patient : null;
  return isDesktop ? startDesktopRecording(speakers, device, p) : startBrowserRecording(speakers, p, onLevel);
}

/** "2026-10-17 19:20" from an engine timestamp. */
export function fmtStamp(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

// ---- events -----------------------------------------------------------------

type Unlisten = () => void;
type Handlers = {
  segment: Set<(s: Segment) => void>;
  token: Set<(t: string) => void>;
  level: Set<(l: { level: number; seconds: number }) => void>;
  download: Set<(d: { model: string; done: number; total: number }) => void>;
};
const handlers: Handlers = { segment: new Set(), token: new Set(), level: new Set(), download: new Set() };
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
  segment: "transcript-segment",
  token: "note-token",
  level: "audio-level",
  download: "download-progress",
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
