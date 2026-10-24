// The phone window's line to the engine (ADR-0023 section 4). It can ask for
// the call the main window prepared, a one-hour token, and record the call.
// Nothing else: no visits, patients or settings. Desktop: Tauri commands the
// engine only accepts from the phone window. Web mode: this computer's own
// server, with the X-OpenBark header and a WebSocket for the audio.
import { invoke } from "@tauri-apps/api/core";
import type { CallTarget, ConsentOutcome, SessionView } from "../engine";

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  headers.set("x-openbark", "1");
  const res = await fetch(path, { ...init, headers });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error ?? `${res.status} ${res.statusText}`);
  return body as T;
}

export interface Token {
  token: string;
  identity: string;
  expires_at: number;
}

export const pending = () =>
  isDesktop ? invoke<CallTarget | null>("phone_pending") : api<CallTarget | null>("/api/phone/pending");

export const token = () =>
  isDesktop ? invoke<Token>("phone_token") : api<Token>("/api/phone/token", { method: "POST" });

/** A call being recorded on this computer: both sides, interleaved. */
export interface CallRecording {
  /** Interleaved frames: clinic, client, clinic, client... */
  push(frames: Float32Array): void;
  /** "I've read it": the announcement was read. Until then the engine keeps nothing. */
  announced(at: number): Promise<void>;
  /** The client's answer. "declined" deletes what was recorded and returns the visit. */
  consent(outcome: ConsentOutcome, announcedAt: number | null): Promise<SessionView | null>;
  /** Hang up: saves the visit (or, after a decline, its final length). */
  end(durationSecs: number, callSid: string | null): Promise<SessionView | null>;
}

export interface StartCall {
  number: string;
  patient_id: string | null;
  call_sid: string | null;
  sample_rate: number;
  /** Seconds since the client answered: the window kept the audio meanwhile. */
  since_answer: number;
}

export function startRecording(start: StartCall): Promise<CallRecording> {
  return isDesktop ? startDesktop(start) : startWeb(start);
}

async function startDesktop(start: StartCall): Promise<CallRecording> {
  const { sample_rate, since_answer, ...rest } = start;
  await invoke<string>("phone_call_start", { start: { ...rest, since_answer_secs: since_answer }, sampleRate: sample_rate });
  let open = true;
  return {
    push(frames) {
      if (!open) return;
      // Raw bytes, not JSON: about 384 KB a second at 48 kHz.
      void invoke("phone_audio", new Uint8Array(frames.buffer, frames.byteOffset, frames.byteLength)).catch(() => {});
    },
    async announced(at) {
      await invoke("phone_call_announced", { at });
    },
    async consent(outcome, announcedAt) {
      const view = await invoke<SessionView | null>("phone_call_consent", { outcome, announcedAt });
      if (outcome === "declined") open = false;
      return view;
    },
    async end(durationSecs, callSid) {
      open = false;
      return invoke<SessionView | null>("phone_call_end", { end: { duration_secs: durationSecs, call_sid: callSid } });
    },
  };
}

async function startWeb(start: StartCall): Promise<CallRecording> {
  const q = new URLSearchParams({
    number: start.number,
    sample_rate: String(start.sample_rate),
    since_answer: String(Math.max(0, start.since_answer)),
    ...(start.patient_id ? { patient_id: start.patient_id } : {}),
    ...(start.call_sid ? { call_sid: start.call_sid } : {}),
  });
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/api/phone/live?${q}`);
  ws.binaryType = "arraybuffer";
  // Replies come back in order: each waiting request takes the next one.
  const waiting: ((msg: Record<string, unknown>) => void)[] = [];
  let closed = false;
  await new Promise<void>((resolve, reject) => {
    ws.onmessage = (e) => {
      const msg = JSON.parse(String(e.data)) as { started?: string; error?: string };
      if (msg.error) reject(new Error(msg.error));
      else resolve();
    };
    ws.onerror = () => reject(new Error("Couldn't reach the OpenBark engine. Check that `openbark serve` is running."));
  });
  ws.onmessage = (e) => waiting.shift()?.(JSON.parse(String(e.data)) as Record<string, unknown>);
  ws.onclose = () => {
    closed = true;
    while (waiting.length) waiting.shift()?.({ error: "The connection to OpenBark closed." });
  };
  const ask = (msg: unknown) =>
    new Promise<Record<string, unknown>>((resolve) => {
      if (closed) return resolve({ error: "The connection to OpenBark closed." });
      waiting.push(resolve);
      ws.send(JSON.stringify(msg));
    });
  let open = true;
  return {
    push(frames) {
      if (open && ws.readyState === WebSocket.OPEN) ws.send(frames as Float32Array<ArrayBuffer>);
    },
    async announced(at) {
      const reply = await ask({ announced: { at } });
      if (reply.error) throw new Error(String(reply.error));
    },
    async consent(outcome, announcedAt) {
      const reply = await ask({ consent: { outcome, announced_at: announcedAt } });
      if (reply.error) throw new Error(String(reply.error));
      if (outcome === "declined") open = false;
      return (reply.declined as SessionView | undefined) ?? null;
    },
    async end(durationSecs, callSid) {
      open = false;
      const reply = await ask({ end: { duration_secs: durationSecs, call_sid: callSid } });
      if (reply.error) throw new Error(String(reply.error));
      return (reply.session as SessionView | null | undefined) ?? null;
    },
  };
}

/** Tell the main window a call was saved, so it opens the visit. */
export function announceSaved(id: string) {
  if (isDesktop) return; // the engine emits phone-call-saved to the main window
  try {
    const channel = new BroadcastChannel("openbark-phone");
    channel.postMessage({ type: "saved", id });
    channel.close();
  } catch {
    // The visit is under Patients either way.
  }
}
