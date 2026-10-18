import type { LevelSource } from "@openbark/ui";
import { createContext, useContext } from "react";
import type { Route } from "./App";
import type { Status } from "./engine";

/** Per-device preferences (conveniences only; safe to lose). */
export interface Settings {
  labelSpeakers: boolean;
  llm: string;
  device: string | null;
  signer: string;
  draftOnStop: boolean;
  /** Template for new drafts. */
  template: string;
  /** The clinic's rule for recording conversations (M1 item 7). */
  consentRule: "all_parties" | "one_party";
  /** What to read to the client before recording. */
  consentScript: string;
  /** When the one-time consent notice was accepted on this device (ISO time). */
  consentAcknowledgedAt: string | null;
  /** Record how the client consented for each recording (optional step). */
  consentEachTime: boolean;
  /** App language (interface). */
  uiLanguage: string;
  /** Language notes are written in ("en", "fr"…). */
  noteLanguage: string;
  /** Letterhead on PDFs. */
  clinicName: string;
  clinicLines: string;
  pageSize: "letter" | "a4";
  /** What Copy note puts on the clipboard. */
  copyFormat: "plain" | "formatted" | "markdown";
  copyCitations: boolean;
}

/** The app language to start in: the browser's, if we have it. */
function defaultLanguage(): string {
  try {
    const nav = (navigator.language || "en").slice(0, 2).toLowerCase();
    return ["en", "fr", "es", "de", "pt"].includes(nav) ? nav : "en";
  } catch {
    return "en";
  }
}

export const DEFAULT_CONSENT_SCRIPT =
  "I'd like to record our conversation so I can write your pet's medical record accurately. " +
  "The recording stays on this clinic's computer, and you can ask me to stop at any time. Is that okay?";

const DEFAULTS: Settings = {
  labelSpeakers: true,
  llm: "gemma-4-e4b-it-q4_0",
  device: null,
  signer: "",
  draftOnStop: true,
  template: "soap",
  consentRule: "all_parties",
  consentScript: DEFAULT_CONSENT_SCRIPT,
  consentAcknowledgedAt: null,
  consentEachTime: false,
  uiLanguage: defaultLanguage(),
  noteLanguage: "en",
  clinicName: "",
  clinicLines: "",
  pageSize: "letter",
  copyFormat: "plain",
  copyCitations: false,
};

const KEY = "openbark.settings";

export function loadSettings(): Settings {
  try {
    return { ...DEFAULTS, ...(JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings>) };
  } catch {
    return DEFAULTS;
  }
}

export function saveSettings(s: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Non-fatal: settings apply for this session.
  }
}

export interface Ctx {
  route: Route;
  navigate: (r: Route) => void;
  status: Status | null;
  refreshStatus: () => void;
  settings: Settings;
  setSettings: (s: Partial<Settings>) => void;
  toast: (message: string) => void;
  announce: (message: string) => void;
  levels: LevelSource & { push(level: number): void };
}

export const AppContext = createContext<Ctx | null>(null);

export function useApp(): Ctx {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp outside AppContext");
  return ctx;
}

export const fmtTime = (s: number) =>
  `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

export const fmtBytes = (b: number) =>
  b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(0)} MB` : `${(b / 1e3).toFixed(0)} KB`;
