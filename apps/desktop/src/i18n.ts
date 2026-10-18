// Interface languages. Messages are keyed by their English text, so a string
// without a translation simply shows in English. Translations live in
// i18n/messages.ts; `{name}` placeholders are filled from `vars`.
import { useContext } from "react";
import { AppContext } from "./context";
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
const BUILTIN_TEMPLATE_NAMES = new Set(["SOAP note", "Callback note", "Discharge instructions", "Client summary"]);
export function templateName(t: (text: string) => string, name: string): string {
  return BUILTIN_TEMPLATE_NAMES.has(name) ? t(name) : name;
}

/** `t("Record")`, `t("Signed by {name}", { name })` in the current app language. */
export function useT() {
  // Outside the app (component tests) everything is English.
  const lang = useContext(AppContext)?.settings.uiLanguage ?? "en";
  return (text: string, vars?: Vars) => translate(lang, text, vars);
}
