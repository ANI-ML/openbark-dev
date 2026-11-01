/**
 * The desktop setup walkthrough's decisions (ADR-0034), kept apart from the
 * screens so they can be tested: when it opens, what it downloads and in
 * which order, how fast a download is going, and how a measured speed reads.
 */
import type { Settings } from "./context";
import type * as engine from "./engine";
import { isIos } from "./visits";

export const E2B = "gemma-4-e2b-it-q4_0";
export const E4B = "gemma-4-e4b-it-q4_0";

/** The built-in note models, smaller first, by their short names. */
export const NOTE_MODELS = [
  { id: E2B, name: "Gemma 4 E2B" },
  { id: E4B, name: "Gemma 4 E4B" },
] as const;

/** A built-in note model's short name; the catalog name for any other. */
export function shortName(id: string, fallback?: string): string {
  return NOTE_MODELS.find((m) => m.id === id)?.name ?? fallback ?? id;
}

/** The GPU speech pack's download id (os-models gpu_pack::PACK_ID). */
export const GPU_PACK = "gpu-speech-pack";

export type SetupMode = "full" | "check";

/**
 * Whether the setup walkthrough opens, and which: on a computer only, after
 * the consent notice (and, with a team, once someone has chosen who they
 * are). It opens by itself on a first run (models still to download and
 * setup never finished or skipped), and whenever the person asks for it
 * again from Settings, Models or Help.
 */
export function setupMode(
  status: engine.Status | null,
  s: Settings,
  waitingForUser: boolean,
): SetupMode | null {
  if (!status || isIos(status) || !s.consentAcknowledgedAt || waitingForUser) return null;
  if (s.showSetup) return s.showSetup;
  const missing = status.models.some((m) => !m.installed);
  return missing && !s.setupDoneAt ? "full" : null;
}

/**
 * The one-time "Check my setup" card for a computer that was set up before
 * this version: everything is downloaded, setup was never run, and the card
 * wasn't dismissed. It suggests; it never changes a model.
 */
export function checkCardDue(status: engine.Status | null, s: Settings): boolean {
  if (!status || isIos(status) || !s.consentAcknowledgedAt || s.showSetup) return false;
  if (s.setupDoneAt || s.setupCardDismissedAt) return false;
  return status.models.length > 0 && status.models.every((m) => m.installed);
}

/** One thing to download in setup. */
export interface QueueItem {
  id: string;
  name: string;
  size: number;
  kind: "model" | "gpu";
}

/**
 * What setup downloads: the speech models not yet on this computer, the
 * chosen note model if it isn't, and the GPU speech pack when asked for,
 * smallest first. The engine's required list names the default note model;
 * the person's choice replaces it.
 */
export function downloadQueue(
  required: engine.Status["models"],
  rows: engine.ModelRow[],
  chosen: string,
  gpuBytes: number | null,
): QueueItem[] {
  const row = (id: string) => rows.find((r) => r.id === id);
  const items: QueueItem[] = required
    .filter((m) => !m.installed && row(m.id)?.slot !== "llm")
    .map((m) => ({ id: m.id, name: m.name, size: row(m.id)?.size_bytes ?? 0, kind: "model" }));
  const llm = row(chosen);
  if (llm && !llm.installed) items.push({ id: llm.id, name: shortName(llm.id, llm.name), size: llm.size_bytes, kind: "model" });
  if (gpuBytes && gpuBytes > 0) items.push({ id: GPU_PACK, name: "GPU speech pack", size: gpuBytes, kind: "gpu" });
  return items.sort((a, b) => a.size - b.size);
}

/**
 * Speed and time left for a download, from what arrived since it started:
 * null until there is something to go on (three seconds and some bytes).
 */
export function transferEta(
  startedAt: number,
  now: number,
  got: number,
  total: number,
): { bytesPerSecond: number; secsLeft: number } | null {
  const secs = (now - startedAt) / 1000;
  if (secs < 3 || got <= 0) return null;
  const bytesPerSecond = got / secs;
  return { bytesPerSecond, secsLeft: Math.round(Math.max(0, total - got) / bytesPerSecond) };
}

/** Whole minutes for a measured note time: at least one. */
export function noteMinutes(secs: number): number {
  return Math.max(1, Math.round(secs / 60));
}

/**
 * Over this, a note is a long wait, and setup suggests the smaller model to
 * someone who chose the larger one.
 */
export const LONG_NOTE_SECS = 5 * 60;

/** The larger model was chosen and the speed check says its notes take long here. */
export function suggestSmaller(check: engine.SpeedCheck | null, chosen: string): boolean {
  return chosen === E4B && (check?.long_visit_note_secs ?? 0) > LONG_NOTE_SECS;
}

/** A speed check as kept in settings. */
export function savedSpeed(check: engine.SpeedCheck, at = new Date()): NonNullable<Settings["speedCheck"]> {
  return {
    model: check.model,
    read: check.read_tokens_per_second,
    write: check.write_tokens_per_second,
    noteSecs: check.long_visit_note_secs,
    at: at.toISOString(),
  };
}

/** "14:32": a time on the clock, for a projected finish. */
export function clockTime(at: number): string {
  return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** The operating system's name. */
export function systemName(platform: string | undefined): string {
  switch (platform) {
    case "windows":
      return "Windows";
    case "macos":
      return "macOS";
    case "linux":
      return "Linux";
    default:
      return platform ?? "";
  }
}
