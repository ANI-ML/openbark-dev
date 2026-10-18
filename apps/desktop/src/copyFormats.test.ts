import { describe, expect, it } from "vitest";
import { formatNote } from "./copyFormats";
import type { NoteRun, Template } from "./engine";

const template: Template = {
  id: "soap",
  version: "1",
  name: "SOAP note",
  sections: [
    { id: "objective", title: "Objective", parts: [{ key: "vitals", label: "Vitals", kind: "fields", fields: [{ key: "weight", label: "Weight" }, { key: "hr", label: "HR" }] }] },
    { id: "plan", title: "Plan", kind: "medications" },
    { id: "follow", title: "Follow-up", kind: "statements" },
  ],
};

const run = {
  status: "draft",
  template: "soap@1",
  model: "m",
  note: {
    objective: { vitals: { weight: { value: 4.1, unit: "kg", evidence: [18] }, hr: null } },
    plan: [{ drug: "Maropitant", dose: "1 mg/kg", route: "SC", frequency: "once", duration: null, evidence: [34] }],
    follow: [],
  },
  faithfulness: { passed: true, statements_checked: 0, numbers_checked: 0, unsupported_numbers: [], citation_mismatches: [], invalid_evidence: [] },
  stats: { prompt_tokens: 0, completion_tokens: 0, total_ms: 0, tokens_per_second: 0, time_to_first_token_ms: 0 },
} as unknown as NoteRun;

const opts = { header: ["Patient: Miso Tanaka"], addenda: [["Addendum, 2026-10-18, Dr P", "Eating again."]] as [string, string][] };

describe("formatNote", () => {
  it("plain text: capitals for headings, no citations by default, explicit not discussed", () => {
    const { text, html } = formatNote(template, run, { ...opts, format: "plain", citations: false });
    expect(html).toBeUndefined();
    expect(text).toContain("SOAP NOTE\nPatient: Miso Tanaka");
    expect(text).toContain("OBJECTIVE\nVitals:\n- Weight: 4.1 kg");
    expect(text).toContain("- Maropitant: 1 mg/kg, SC, once\n");
    expect(text).toContain("FOLLOW-UP\nNot discussed");
    expect(text).toContain("ADDENDUM, 2026-10-18, DR P\nEating again.");
    expect(text).not.toContain("[18]");
  });

  it("formatted: HTML with escaped text, plus a plain fallback", () => {
    const r = { ...run, note: { ...run.note, follow: [{ text: "Recheck <3 days>", evidence: [1] }] } } as NoteRun;
    const { text, html } = formatNote(template, r, { ...opts, format: "formatted", citations: true });
    expect(html).toContain("<h3>Objective</h3><p><strong>Vitals</strong></p><ul><li>Weight: 4.1 kg [18]</li></ul>");
    expect(html).toContain("<li>Recheck &lt;3 days&gt; [1]</li>");
    expect(text).toContain("Recheck <3 days> [1]");
  });

  it("markdown with a translated not-discussed", () => {
    const { text } = formatNote(template, run, { ...opts, format: "markdown", citations: false, notDiscussed: "Non abordé" });
    expect(text.startsWith("# SOAP note\n\nPatient: Miso Tanaka  \n")).toBe(true);
    expect(text).toContain("## Follow-up\n\n*Non abordé*");
  });

  it("a text box pastes as one paragraph, without bullets", () => {
    const t2: Template = { ...template, sections: [...template.sections, { id: "report", title: "Surgical report", kind: "text" }] };
    const r = {
      ...run,
      note: {
        ...run.note,
        report: [
          { text: "Ventral midline approach.", evidence: [5] },
          { text: "Closed in three layers.", evidence: [7] },
        ],
      },
    } as NoteRun;
    const plain = formatNote(t2, r, { ...opts, format: "plain", citations: false }).text;
    expect(plain).toContain("SURGICAL REPORT\nVentral midline approach. Closed in three layers.");
    const md = formatNote(t2, r, { ...opts, format: "markdown", citations: true }).text;
    expect(md).toContain("## Surgical report\n\nVentral midline approach. [5] Closed in three layers. [7]");
    const { html } = formatNote(t2, r, { ...opts, format: "formatted", citations: false });
    expect(html).toContain("<h3>Surgical report</h3><p>Ventral midline approach. Closed in three layers.</p>");
  });
});
