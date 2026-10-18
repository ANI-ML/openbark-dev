// Deleting visits and recordings (ADR-0022): shared wording.
import type { ReasonKind } from "./engine";

type T = (text: string, vars?: Record<string, string | number>) => string;

/** The reason menu, in its order. */
export const REASON_KINDS: ReasonKind[] = ["test", "mistake", "duplicate", "consent_withdrawn", "other"];

/** A deletion reason as the menu and Deleted visits show it. */
export function reasonLabel(t: T, kind: ReasonKind): string {
  switch (kind) {
    case "test":
      return t("Test or practice recording");
    case "mistake":
      return t("Recorded by mistake");
    case "duplicate":
      return t("Duplicate visit");
    case "consent_withdrawn":
      return t("Client withdrew consent");
    default:
      return t("Another reason");
  }
}

/** "Olive and Pickles", in the app language. */
export function listNames(lang: string, names: string[]): string {
  try {
    return new Intl.ListFormat(lang, { style: "long", type: "conjunction" }).format(names);
  } catch {
    return names.join(", ");
  }
}

/** The patient record of one animal in a visit (the main animal's, or another's). */
export function patientIdFor(
  patient: { name: string; others?: string[]; patient_id?: string | null; other_ids?: string[] } | null | undefined,
  pet: string | null | undefined,
): string | null {
  if (!patient) return null;
  if (pet && pet.trim().toLowerCase() !== patient.name.trim().toLowerCase()) {
    const i = (patient.others ?? []).findIndex((o) => o.trim().toLowerCase() === pet.trim().toLowerCase());
    if (i >= 0) return patient.other_ids?.[i] ?? null;
  }
  return patient.patient_id ?? null;
}
