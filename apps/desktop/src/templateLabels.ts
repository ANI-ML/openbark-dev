// Words for template versions (ADR-0024): list badges, the editor's notice,
// and the label that says which version drafted a note. Pure functions, so
// every state can be tested. In a team the shared layer is "the hospital's".
import type { Origin, OriginalChanged, Template, TemplateSummary, TemplateView, TemplatesMode } from "./engine";
import { templateName, type Vars } from "./i18n";

type T = (text: string, vars?: Vars) => string;

export interface Badge {
  text: string;
  tone: "neutral" | "warn";
}

/** A change to the original worth a "Newer original" badge (not one that's gone). */
export function isNewerOriginal(change: OriginalChanged | null | undefined): boolean {
  return !!change && ["newer_builtin", "team_changed", "team_reset"].includes(change.reason);
}

/** Badges for a row of the template list (ADR-0024 section 14.3): a dot with text, never colour alone. */
export function badges(t: T, s: TemplateSummary, mode: TemplatesMode): Badge[] {
  const out: Badge[] = [];
  if (s.layer === "personal") out.push({ text: s.edited ? t("Your version") : t("Only you"), tone: "neutral" });
  else if (s.layer === "team" && s.builtin) out.push({ text: t("Edited"), tone: "neutral" });
  else if (s.layer === "team") out.push({ text: mode === "alone" ? t("Yours") : t("Hospital"), tone: "neutral" });
  if (isNewerOriginal(s.original_changed)) out.push({ text: t("Newer original"), tone: "warn" });
  return out;
}

/** "SOAP note, your version 3": which version drafted a note (ADR-0024 section 10). */
export function draftLabel(t: T, template: Template | null | undefined, me: string | null | undefined, mode: TemplatesMode): string | null {
  const origin: Origin | null | undefined = template?.origin;
  if (!template || !origin) return null; // drafted before versions were recorded
  const name = templateName(t, template.name);
  const n = origin.version;
  if (origin.layer === "builtin") return t("{name} {version}", { name, version: n });
  if (origin.layer === "personal") {
    if (origin.owner?.id && origin.owner.id === me) return t("{name}, your version {n}", { name, n });
    return t("{name}, {person}'s version {n}", { name, person: origin.owner?.name ?? "", n });
  }
  if (mode === "alone") return origin.based_on ? t("{name}, edited version {n}", { name, n }) : t("{name} {version}", { name, version: n });
  return t("{name}, the hospital's version {n}", { name, n });
}

/** The notice under the editor's title, for each state (ADR-0024 section 14.2). */
export function editorNotice(t: T, view: TemplateView | null, isNew: boolean): string | null {
  if (!view || isNew) return null;
  const { can, layer, mode, origin, original } = view;
  const name = templateName(t, original?.name ?? view.template.name);
  if (mode === "hospital") {
    return can.edit
      ? t("Templates are shared by the hospital. Changes you save apply to everyone and need your PIN.")
      : t("Templates are shared by the hospital, so an admin changes them. You can read every template and what the model is told.");
  }
  if (!can.edit) return t("Templates are changed by the people who draft notes. You can read every template and what the model is told.");
  if (mode === "alone") {
    if (layer === "builtin") return t("Changes are saved as your version. Reset to original brings back the built-in at any time.");
    if (layer === "team" && original) return t("Your edited version {n}, from {name} {version}.", { n: view.template.version, name, version: origin?.based_on?.version ?? original.version });
    return null;
  }
  if (layer === "personal" && !original) return t("Only you draft with this template.");
  if (layer === "personal") {
    return t("Your version {n}, from {name} {version}. Only you draft with it.", {
      n: view.template.version,
      name,
      version: origin?.based_on?.version ?? original?.version ?? "",
    });
  }
  if (can.save_to === "team") return t("Changes you save here go to the hospital's templates and need your PIN.");
  if (layer === "team" && view.builtin) {
    return t("The hospital's version {n} of {name}. Changes are saved as your version.", { n: view.template.version, name });
  }
  return t("Changes are saved as your version. Everyone else keeps the original.");
}

/** What happened to the original (the "Newer original" notice). */
export function originalNotice(t: T, view: TemplateView): string | null {
  const c = view.original_changed;
  if (!c) return null;
  const name = templateName(t, c.name);
  switch (c.reason) {
    case "newer_builtin":
      return t("A newer {name} is built in (version {version}). Your version is unchanged.", { name, version: c.version });
    case "team_changed":
      return t("The hospital changed {name} (hospital version {n}). Your version is unchanged.", { name, n: c.version });
    case "team_reset":
      return t("The hospital's version of {name} was reset to the built-in. Your version is unchanged.", { name });
    case "team_deleted":
      return t("The hospital template this came from was deleted. Your version still works.");
    case "no_longer_builtin":
      return t("This template is no longer built in. Your version still works.");
  }
}

/** "Based on an earlier SOAP note": kept after Keep mine. */
export function keptNotice(t: T, view: TemplateView): string | null {
  if (view.original_changed || !view.origin?.seen_original || !view.original) return null;
  return t("Based on an earlier {name}.", { name: templateName(t, view.original.name) });
}

/** The version shown beside the editor's title. */
export function versionLabel(t: T, view: TemplateView): string {
  const v = view.template.version;
  if (view.layer === "personal" && view.original) return t("your version {n}", { n: v });
  if (view.layer === "team" && view.builtin && view.mode !== "alone") return t("the hospital's version {n}", { n: v });
  return t("version {v}", { v });
}
