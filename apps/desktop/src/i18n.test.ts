import { describe, expect, it } from "vitest";
import { BUILTIN_TEXT } from "./builtinText";
import { builtinText, hardwareLine, sourceLabel, translate, UI_LANGUAGES, withoutContext } from "./i18n";
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

describe("built-in template text", () => {
  // The YAML of every built-in template, read as text (no YAML parser in the app).
  const yamls = Object.values(
    import.meta.glob("../../../packages/templates/*/template.yaml", { query: "?raw", import: "default", eager: true }),
  ) as string[];
  const unquote = (s: string) => s.trim().replace(/^"(.*)"$/, "$1");
  const strings = new Set<string>();
  for (const yaml of yamls) {
    const lines = yaml.split("\n");
    lines.forEach((line, i) => {
      const title = line.match(/^\s*(?:- )?(?:title|label):\s*(.+?)\s*$/);
      if (title?.[1]) strings.add(unquote(title[1]));
      for (const m of line.matchAll(/\blabel:\s*([^,}]+?)\s*[,}]/g)) strings.add(unquote(m[1] ?? ""));
      if (/^\s*examples:\s*$/.test(line)) {
        for (let j = i + 1; j < lines.length; j++) {
          const item = lines[j]?.match(/^\s*- "(.*)"\s*$/);
          if (!item?.[1]) break;
          strings.add(item[1]);
        }
      }
    });
  }

  it("lists every heading, measurement and example of the built-in templates", () => {
    expect(yamls.length).toBeGreaterThan(10);
    const missing = [...strings].filter((s) => !BUILTIN_TEXT.has(s));
    expect(missing).toEqual([]);
  });

  it("has every built-in string translated in every interface language", () => {
    for (const { code } of UI_LANGUAGES.filter((l) => l.code !== "en")) {
      const missing = [...BUILTIN_TEXT].filter((s) => !MESSAGES[code]?.[s]);
      expect(missing, code).toEqual([]);
    }
  });

  it("translates a built-in template's text, never a team's own", () => {
    const t = (s: string) => translate("fr", s);
    expect(builtinText(t, "Presenting complaint", true)).toBe("Motif de consultation");
    expect(builtinText(t, "Presenting complaint", false)).toBe("Presenting complaint");
    expect(builtinText(t, "Our own heading", true)).toBe("Our own heading");
  });
});

describe("i18n helpers", () => {
  it("keeps the two senses of Standard apart", () => {
    expect(withoutContext(translate("fr", "Standard (level of detail)"))).toBe("Standard");
    expect(withoutContext(translate("en", "Standard (level of detail)"))).toBe("Standard");
    expect(translate("fr", "Standard")).toBe("Norme");
  });

  it("says where a visit came from in the app language", () => {
    const t = (s: string) => translate("de", s);
    expect(sourceLabel(t, "Recorded")).toBe("Live-Aufnahme");
    expect(sourceLabel(t, "")).toBe("Live-Aufnahme");
    expect(sourceLabel(t, "feline-vomiting.wav")).toBe("feline-vomiting.wav");
  });

  it("builds the hardware line from the structured fields", () => {
    const t = (s: string, v?: Record<string, string | number>) => translate("fr", s, v);
    expect(hardwareLine(t, { gpu: null, gpu_memory_bytes: null, ram_bytes: 68.7e9, summary: "" })).toBe(
      "69 Go de mémoire, aucune carte graphique compatible",
    );
    expect(hardwareLine(t, { gpu: "RTX 4080", gpu_memory_bytes: 16e9, ram_bytes: 0, summary: "" })).toBe(
      "RTX 4080, 16 Go de mémoire graphique",
    );
  });
});
