import { describe, expect, it } from "vitest";
import type { SessionSummary, Status } from "./engine";
import { translate } from "./i18n";
import { hashToRoute, routeToHash, type Route } from "./routes";
import { animalFamily, firstWhen, inWhen, recordedLine, recordedOn, visitStatus, whenLabel } from "./visits";

const t = (s: string, v?: Record<string, string | number>) => translate("en", s, v);
const now = new Date(2026, 9, 24, 15, 0); // Saturday 24 October 2026, 15:00

const visit = (over: Partial<SessionSummary> = {}): SessionSummary => ({
  id: "v",
  created_at: "2026-10-24 10:42",
  source: "Recorded",
  audio_secs: 728,
  runs: 0,
  signed: false,
  patient: "Maple Okonkwo",
  reason: "Annual wellness",
  template: "",
  ...over,
});

describe("times as people say them (ADR-0027)", () => {
  it("shows the time today, Yesterday and the time, then the day", () => {
    expect(whenLabel("2026-10-24 10:42", now, "en", t)).toBe("10:42");
    expect(whenLabel("2026-10-23 16:05", now, "en", t)).toBe("Yesterday 16:05");
    expect(whenLabel("2026-10-19 09:00", now, "en", t)).toBe("Mon 19 Oct");
    expect(whenLabel("2025-12-30 09:00", now, "en", t)).toBe("Tue 30 Dec 2025");
    expect(whenLabel("not a date", now, "en", t)).toBe("not a date");
  });

  it("says when and where a visit was recorded in one sentence", () => {
    expect(recordedLine("2026-10-24 10:42", now, "en", "this computer", t)).toBe(
      "Recorded today at 10:42 on this computer.",
    );
    expect(recordedLine("2026-10-23 08:10", now, "en", "this iPhone", t)).toBe(
      "Recorded yesterday at 08:10 on this iPhone.",
    );
  });

  it("files visits under Today, This week and Earlier, and opens on the first with something in it", () => {
    expect(inWhen("2026-10-24 08:00", "today", now)).toBe(true);
    expect(inWhen("2026-10-20 08:00", "week", now)).toBe(true);
    expect(inWhen("2026-10-20 08:00", "today", now)).toBe(false);
    expect(inWhen("2026-10-10 08:00", "earlier", now)).toBe(true);
    expect(firstWhen([visit({ created_at: "2026-10-21 08:00" })], now)).toBe("week");
    expect(firstWhen([visit()], now)).toBe("today");
  });
});

describe("one status per visit", () => {
  const desktop = { profile: "desktop" } as Status;
  const ios = { profile: "ios" } as Status;
  it("names who signed, and what's next otherwise", () => {
    expect(visitStatus(visit({ signed: true, signed_by: "Dr Rivera" }), desktop, t)).toEqual({
      tone: "ok",
      text: "Signed by Dr Rivera",
    });
    expect(visitStatus(visit({ runs: 1 }), desktop, t).text).toBe("Check and sign");
    // No signing on the phone during the pilot.
    expect(visitStatus(visit({ runs: 1 }), ios, t).text).toBe("Check note");
    expect(visitStatus(visit(), desktop, t).text).toBe("Ready to draft");
    // An iPhone that leaves drafting to a computer.
    expect(visitStatus(visit(), ios, t, false).text).toBe("Transcribed");
  });

  it("puts the middle dot only between animal and family", () => {
    expect(animalFamily({ animal: "Maple", family: "Okonkwo" })).toBe("Maple · Okonkwo");
    expect(animalFamily({ animal: "Maple", family: "" })).toBe("Maple");
    expect(animalFamily({ patient: "Maple Okonkwo" })).toBe("Maple Okonkwo");
  });

  it("says which device a visit was recorded on", () => {
    expect(recordedOn(visit(), desktop, t)).toBe("This computer");
    expect(recordedOn(visit(), ios, t)).toBe("This iPhone");
    expect(recordedOn(visit({ audio_on: "Front desk Mac" }), desktop, t)).toBe("Front desk Mac");
  });
});

describe("stable routes", () => {
  it("round-trips each destination through the hash, without one-off instructions", () => {
    const routes: Route[] = [
      { screen: "record" },
      { screen: "visits" },
      { screen: "review", id: "2026-10-24_104200_maple", pet: "Pickles" },
      { screen: "patients" },
      { screen: "patient", id: "p-1" },
      { screen: "templates", open: "soap" },
      { screen: "models" },
      { screen: "help" },
      { screen: "settings" },
      { screen: "settings", section: "recording" },
    ];
    for (const r of routes) expect(hashToRoute(routeToHash(r))).toEqual(r);
    expect(routeToHash({ screen: "review", id: "v1", draft: true })).toBe("#/visits/v1");
    expect(hashToRoute("#/settings/nonsense")).toEqual({ screen: "settings" });
    expect(hashToRoute("")).toEqual({ screen: "record" });
  });
});
