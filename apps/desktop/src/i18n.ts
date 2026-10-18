// Interface languages. Messages are keyed by their English text, so a string
// without a translation simply shows in English. Translations live in
// i18n/messages.ts; `{name}` placeholders are filled from `vars`.
import { useContext } from "react";
import { AppContext } from "./context";
import { BUILTIN_TEXT } from "./builtinText";
import type { Hardware } from "./engine";
import { MESSAGES } from "./i18n/messages";

export const UI_LANGUAGES = [
  { code: "en", name: "English" },
  { code: "fr", name: "Français" },
  { code: "es", name: "Español" },
  { code: "de", name: "Deutsch" },
  { code: "pt", name: "Português" },
] as const;

/** Languages the note model can write in (engine NOTE_LANGUAGES). */
export const NOTE_LANGUAGES = [
  { code: "en", name: "English" },
  { code: "fr", name: "Français" },
  { code: "es", name: "Español" },
  { code: "de", name: "Deutsch" },
  { code: "pt", name: "Português" },
  { code: "it", name: "Italiano" },
  { code: "nl", name: "Nederlands" },
] as const;

export type Vars = Record<string, string | number>;

export function translate(lang: string, text: string, vars?: Vars): string {
  const out = (lang !== "en" && MESSAGES[lang]?.[text]) || text;
  return vars ? out.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : out;
}

/** Built-in templates' names are translated; a team's own names are shown as written. */
const BUILTIN_TEMPLATE_NAMES = new Set([
  "SOAP note",
  "Phone call",
  "Annual exam",
  "New consult",
  "Recheck",
  "Results",
  "Clinician rounds",
  "Discharge instructions",
  "Client summary",
  "Emergency and triage",
  "Dental procedure",
  "Surgery and anaesthesia",
  "Dermatology consult",
  "Oncology visit",
  "Exotics consult",
  "Equine visit",
  "Production animal visit",
  "Euthanasia and quality of life",
  "Estimate explanation",
  "Euthanasia aftercare",
]);
export function templateName(t: (text: string) => string, name: string): string {
  return BUILTIN_TEMPLATE_NAMES.has(name) ? t(name) : name;
}

/** A built-in template's description is translated too; a team's own is shown as written. */
export function templateDescription(
  t: (text: string) => string,
  tpl: { description?: string | null; builtin?: boolean },
): string {
  const text = tpl.description ?? "";
  return tpl.builtin && text ? t(text) : text;
}

type T = (text: string, vars?: Vars) => string;

/**
 * A built-in template's heading titles, measurement labels and example
 * requests are translated (their keys are listed in builtinText.ts); a team's
 * own are shown as written. The model's instructions stay in English.
 */
export function builtinText(t: (text: string) => string, text: string, builtin: boolean): string {
  return builtin && text && BUILTIN_TEXT.has(text) ? t(text) : text;
}

/**
 * Keys such as "Standard (level of detail)" carry their meaning in brackets so
 * translators can tell two senses of one word apart; untranslated (English),
 * the bracket is dropped: `withoutContext(t("Standard (level of detail)"))`.
 */
export function withoutContext(text: string): string {
  return text.replace(/ \([^)]*\)$/, "");
}

/** The computer's hardware in the app language (the engine's `summary` is English, for logs). */
export function hardwareLine(t: T, hw: Hardware | null | undefined): string {
  if (!hw) return "";
  const gb = (bytes: number) => Math.round(bytes / 1e9);
  return hw.gpu
    ? t("{gpu}, {gb} GB graphics memory", { gpu: hw.gpu, gb: gb(hw.gpu_memory_bytes ?? 0) })
    : t("{gb} GB memory, no supported graphics card", { gb: gb(hw.ram_bytes) });
}

/** Where a visit came from: an imported file's name, or a live recording (the engine says "Recorded"). */
export function sourceLabel(t: T, source: string | null | undefined): string {
  return !source || source === "Recorded" ? t("Live recording") : source;
}

/** `t("Record")`, `t("Signed by {name}", { name })` in the current app language. */
export function useT() {
  // Outside the app (component tests) everything is English.
  const lang = useContext(AppContext)?.settings.uiLanguage ?? "en";
  return (text: string, vars?: Vars) => translate(lang, text, vars);
}
