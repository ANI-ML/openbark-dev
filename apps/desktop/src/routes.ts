/**
 * Where the app is (ADR-0027). Each destination has a stable hash route, so
 * Back works in a browser, in the desktop window and in the iPhone's web
 * view, and a reload opens the same screen. One-off instructions (draft on
 * open, try a template) travel only in memory, never in the hash.
 */

/** Settings, one section at a time (desktop) or one sub-screen per row (phones). */
export const SETTINGS_SECTIONS = [
  "you",
  "recording",
  "vocabulary",
  "language",
  "phone",
  "pdf",
  "sync",
  "hub",
  "privacy",
  "deleted",
  "about",
] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export type Route =
  | { screen: "record"; patient?: string }
  | { screen: "visits" }
  | { screen: "patients" }
  | { screen: "patient"; id: string }
  | { screen: "review"; id: string; draft?: boolean; template?: string; pet?: string }
  | { screen: "templates"; open?: string; heading?: string }
  | { screen: "help" }
  | { screen: "models" }
  | { screen: "settings"; section?: SettingsSection };

const enc = encodeURIComponent;

/** The route as a hash: "#/visits", "#/visits/<id>", "#/settings/recording". */
export function routeToHash(r: Route): string {
  switch (r.screen) {
    case "record":
      return r.patient ? `#/record?patient=${enc(r.patient)}` : "#/record";
    case "patient":
      return `#/patients/${enc(r.id)}`;
    case "review":
      return `#/visits/${enc(r.id)}${r.pet ? `?pet=${enc(r.pet)}` : ""}`;
    case "templates":
      return r.open ? `#/templates/${enc(r.open)}` : "#/templates";
    case "settings":
      return r.section ? `#/settings/${r.section}` : "#/settings";
    default:
      return `#/${r.screen}`;
  }
}

/** The route a hash names; Record for anything else. */
export function hashToRoute(hash: string): Route {
  const [path = "", query = ""] = hash.replace(/^#\/?/, "").split("?");
  const params = new URLSearchParams(query);
  const [screen = "", id] = path.split("/").map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p;
    }
  });
  switch (screen) {
    case "record":
      return params.get("patient") ? { screen: "record", patient: params.get("patient")! } : { screen: "record" };
    case "visits":
      if (!id) return { screen: "visits" };
      return params.get("pet") ? { screen: "review", id, pet: params.get("pet")! } : { screen: "review", id };
    case "patients":
      return id ? { screen: "patient", id } : { screen: "patients" };
    case "templates":
      return id ? { screen: "templates", open: id } : { screen: "templates" };
    case "help":
    case "models":
      return { screen };
    case "settings":
      return (SETTINGS_SECTIONS as readonly string[]).includes(id ?? "")
        ? { screen: "settings", section: id as SettingsSection }
        : { screen: "settings" };
    default:
      return { screen: "record" };
  }
}
