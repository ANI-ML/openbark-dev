import type { LevelSource } from "@openbark/ui";
import { createContext, useContext } from "react";
import type { Route } from "./routes";
import type { Role, Status, User } from "./engine";

/** Per-device preferences (conveniences only; safe to lose). */
export interface Settings {
  labelSpeakers: boolean;
  /** A client on a phone on speaker in the room: pick up the quieter voice (ADR-0026). */
  phoneOnSpeaker: boolean;
  /**
   * Speakers in the room (ADR-0030): how many people talk in a consult, or
   * null (Auto) to let the engine estimate it when recording stops.
   */
  roomSpeakers: number | null;
  llm: string;
  /** The desktop microphone, by name; null is the system default. */
  device: string | null;
  /** Web mode: the browser's microphone, by its device id; null is the system default. */
  browserMic: string | null;
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
  /**
   * The language spoken in consultations ("fr"), or null for Auto: each
   * sentence's language is detected, and the second listen expects the app's
   * language, as before 0.2.5 (ADR-0033). Separate from the app's language:
   * a clinic can use the app in French and speak a mix.
   */
  spokenLanguage: string | null;
  /** Language notes are written in ("en", "fr"…). */
  noteLanguage: string;
  /** Letterhead on PDFs. */
  clinicName: string;
  clinicLines: string;
  pageSize: "letter" | "a4";
  /** What Copy note puts on the clipboard. */
  copyFormat: "plain" | "formatted" | "markdown";
  copyCitations: boolean;
  /**
   * iPhone profile: draft notes on the phone itself, or leave drafting to a
   * computer (ADR-0027). Off until the person turns it on: drafting on the
   * phone is opt-in (decision of 2026-10-19, ADR-0018 section 10), and
   * iPhones with less than 8 GB never draft.
   */
  writeNotesHere: boolean;
  /** iPhone profile: when the first-run walkthrough was finished or skipped. */
  walkthroughDoneAt: string | null;
  /** iPhone profile: show the walkthrough again (Settings, Help). */
  showWalkthrough: boolean;
  /**
   * The daily version check (ADR-0035): "ask" until the person answers the
   * one-time question, then "on" or "off". Only "on" checks by itself;
   * "ask" checks nothing, so ignoring the question leaves it off. Any place
   * that asks the question (the card, setup) sets this; it is the one
   * source of truth. Never used on the iPhone.
   */
  updateCheck: "ask" | "on" | "off";
  /** When the one-time question was shown on this device (ISO time): it isn't shown again. */
  updateAskedAt: string | null;
  /** The newest version the "is available" notice was shown for: once per version. */
  updateNotified: string | null;
  /** Computer: when the setup walkthrough was finished or skipped (ADR-0034). */
  setupDoneAt: string | null;
  /** Computer: when the one-time "Check my setup" card was dismissed or used. */
  setupCardDismissedAt: string | null;
  /** Computer: open the setup walkthrough again, the whole of it or the check (Settings, Models, Help). */
  showSetup: "full" | "check" | null;
  /**
   * When the person chose the note model in setup: the app then never swaps
   * it for the computer's default on its own (App.tsx).
   */
  noteModelChosenAt: string | null;
  /** The last speed check on this computer (ADR-0034), for Models and Record. */
  speedCheck: SavedSpeedCheck | null;
}

/** A speed check's result, kept on this device. */
export interface SavedSpeedCheck {
  model: string;
  /** Tokens a second, reading and writing. */
  read: number;
  write: number;
  /** A long visit's note at these speeds, in seconds. */
  noteSecs: number | null;
  at: string;
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
  phoneOnSpeaker: false,
  roomSpeakers: null,
  llm: "gemma-4-e4b-it-q4_0",
  device: null,
  browserMic: null,
  signer: "",
  draftOnStop: true,
  template: "soap",
  consentRule: "all_parties",
  consentScript: DEFAULT_CONSENT_SCRIPT,
  consentAcknowledgedAt: null,
  consentEachTime: false,
  uiLanguage: defaultLanguage(),
  spokenLanguage: null,
  noteLanguage: "en",
  clinicName: "",
  clinicLines: "",
  pageSize: "letter",
  copyFormat: "plain",
  copyCitations: false,
  writeNotesHere: false,
  walkthroughDoneAt: null,
  showWalkthrough: false,
  updateCheck: "ask",
  updateAskedAt: null,
  updateNotified: null,
  setupDoneAt: null,
  setupCardDismissedAt: null,
  showSetup: null,
  noteModelChosenAt: null,
  speedCheck: null,
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

/** A toast's one action, such as Undo. */
export interface ToastAction {
  label: string;
  run: () => void;
}

/** More for a toast: buttons after `action`, and how long it stays (4 seconds unless set). */
export interface ToastOptions {
  more?: ToastAction[];
  ms?: number;
}

export interface Ctx {
  route: Route;
  navigate: (r: Route) => void;
  status: Status | null;
  refreshStatus: () => void;
  settings: Settings;
  setSettings: (s: Partial<Settings>) => void;
  /** Show a toast; `action` adds a button (Undo); `options` more buttons and a longer stay. */
  toast: (message: string, action?: ToastAction, options?: ToastOptions) => void;
  announce: (message: string) => void;
  levels: LevelSource & { push(level: number): void };
  /** The team member using the app (null single-user, or before choosing). */
  user: User | null;
  setUser: (u: User | null) => void;
  /** Sent as `by`: the team member's id, or the name from Settings. */
  actor: string;
  /** The acting person's name, for messages. */
  actorName: string;
  /** What the person using the app may do. `admin`: changes the hospital's templates, with their PIN. */
  can: { notes: boolean; sign: boolean; team: boolean; admin: boolean };
}

/** Permissions for a role; single-user (no role) may do everything. */
export function permissions(role: Role | null | undefined, admin = false) {
  return {
    notes: !role || role === "dvm" || role === "rvt",
    sign: !role || role === "dvm",
    team: !role || role === "dvm",
    admin: !role || admin,
  };
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
