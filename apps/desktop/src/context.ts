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
}

const DEFAULTS: Settings = {
  labelSpeakers: true,
  llm: "gemma-4-e4b-it-q4_0",
  device: null,
  signer: "",
  draftOnStop: true,
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
