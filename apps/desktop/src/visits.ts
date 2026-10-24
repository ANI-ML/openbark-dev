import type { Tone } from "@openbark/ui";
import type { SessionSummary, Status } from "./engine";

/**
 * Visit helpers for the redesign (ADR-0027): times as people say them,
 * Today / This week / Earlier, one status per visit, and which device a
 * visit was recorded on. Pure, so they're tested without a screen.
 */

type Translate = (text: string, vars?: Record<string, string | number>) => string;

/** "2026-10-24 10:42" (the engine's local time) as a Date; null if it doesn't parse. */
export function parseStamp(stamp: string): Date | null {
  const m = stamp.match(/^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/);
  if (!m) return null;
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), Number(m[4]), Number(m[5]));
}

const DAY = 24 * 60 * 60 * 1000;
const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
/** Whole calendar days from `d` to `now` (0 today, 1 yesterday). */
function daysAgo(d: Date, now: Date): number {
  return Math.round((startOfDay(now).getTime() - startOfDay(d).getTime()) / DAY);
}

/** The locale for dates: British English for English, so it reads "Mon 21 Oct". */
const locale = (lang: string) => (lang === "en" ? "en-GB" : lang);
const hhmm = (d: Date) => `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

/** "Mon 21 Oct", with the year when it isn't this year. */
export function dayLabel(d: Date, now: Date, lang: string): string {
  const opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short" };
  if (d.getFullYear() !== now.getFullYear()) opts.year = "numeric";
  return new Intl.DateTimeFormat(locale(lang), opts).format(d).replace(/,/g, "");
}

/** "10:42" today, "Yesterday 16:05", "Mon 21 Oct" before that; the raw stamp if it doesn't parse. */
export function whenLabel(stamp: string, now: Date, lang: string, t: Translate): string {
  const d = parseStamp(stamp);
  if (!d) return stamp;
  const ago = daysAgo(d, now);
  if (ago <= 0) return hhmm(d);
  if (ago === 1) return t("Yesterday {time}", { time: hhmm(d) });
  return dayLabel(d, now, lang);
}

/** "Recorded today at 10:42 on this computer." for Review's meta line. */
export function recordedLine(stamp: string, now: Date, lang: string, device: string, t: Translate): string {
  const d = parseStamp(stamp);
  if (!d) return t("Recorded {date} on {device}.", { date: stamp, device });
  const ago = daysAgo(d, now);
  if (ago <= 0) return t("Recorded today at {time} on {device}.", { time: hhmm(d), device });
  if (ago === 1) return t("Recorded yesterday at {time} on {device}.", { time: hhmm(d), device });
  return t("Recorded {date} at {time} on {device}.", { date: dayLabel(d, now, lang), time: hhmm(d), device });
}

export type When = "today" | "week" | "earlier";

/** Today; within the last 7 days (today included); or earlier. */
export function inWhen(stamp: string, when: When, now: Date): boolean {
  const d = parseStamp(stamp);
  if (!d) return when === "earlier";
  const ago = daysAgo(d, now);
  if (when === "today") return ago <= 0;
  if (when === "week") return ago < 7;
  return ago >= 7;
}

/** The first filter that has something in it, so the list never opens empty. */
export function firstWhen(rows: SessionSummary[], now: Date): When {
  if (rows.some((r) => inWhen(r.created_at, "today", now))) return "today";
  if (rows.some((r) => inWhen(r.created_at, "week", now))) return "week";
  return rows.length ? "earlier" : "today";
}

/** A visit nobody has signed still needs someone. */
export const needsYou = (r: SessionSummary) => !r.signed;

/** The phone profile (ADR-0018 section 10): no signing during the pilot (ADR-0027). */
export const isIos = (status: Status | null | undefined) => status?.profile === "ios";

/** "this iPhone" or "this computer", already translated. */
export function thisDevice(status: Status | null | undefined, t: Translate): string {
  return isIos(status) ? t("this iPhone") : t("this computer");
}

/** One status per visit: a dot and its words (colour never alone). */
export function visitStatus(r: SessionSummary, status: Status | null | undefined, t: Translate): { tone: Tone; text: string } {
  if (r.signed) return { tone: "ok", text: r.signed_by ? t("Signed by {name}", { name: r.signed_by }) : t("Signed") };
  if (r.runs > 0) return { tone: "warn", text: isIos(status) ? t("Check note") : t("Check and sign") };
  return { tone: "neutral", text: t("Ready to draft") };
}

/** "Maple · Okonkwo" (the one place a middle dot belongs), or the combined name from older engines. */
export function animalFamily(r: { animal?: string; family?: string; patient?: string }): string {
  const animal = r.animal?.trim() ?? "";
  const family = r.family?.trim() ?? "";
  if (animal && family) return `${animal} · ${family}`;
  return animal || family || r.patient?.trim() || "";
}

/** Where the visit was recorded: another device's name, or this one. */
export function recordedOn(r: SessionSummary, status: Status | null | undefined, t: Translate): string {
  if (r.audio_on) return r.audio_on;
  return isIos(status) ? t("This iPhone") : t("This computer");
}
