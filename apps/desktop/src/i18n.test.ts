import { describe, expect, it } from "vitest";
import { translate, UI_LANGUAGES } from "./i18n";
import { MESSAGES } from "./i18n/messages";

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("i18n", () => {
  it("falls back to English and fills placeholders", () => {
    expect(translate("en", "Signed by {name}, {at}", { name: "Dr P", at: "today" })).toBe("Signed by Dr P, today");
    expect(translate("xx", "A string nobody translated")).toBe("A string nobody translated");
  });

  it("every translation keeps the English placeholders", () => {
    for (const [lang, catalog] of Object.entries(MESSAGES)) {
      for (const [english, translated] of Object.entries(catalog)) {
        expect(placeholders(translated), `${lang}: ${english}`).toEqual(placeholders(english));
      }
    }
  });

  it("has a catalog for every offered interface language", () => {
    for (const { code } of UI_LANGUAGES.filter((l) => l.code !== "en")) {
      expect(Object.keys(MESSAGES[code] ?? {}).length, code).toBeGreaterThan(100);
    }
  });
});
