import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { formatNote } from "../copyFormats";
import type { NoteRun, Template } from "../engine";
import { NoteView } from "./NoteView";

afterEach(cleanup);

// A dental note with a tooth chart heading (ADR-0021), as the engine sends it.
const template = (dentition: "canine" | "feline" | null): Template => ({
  id: "dental",
  version: "0.2.0",
  name: "Dental procedure",
  sections: [
    { id: "dental_chart", title: "Dental chart", kind: "teeth", dentition, parts: [] },
    { id: "whole_mouth_findings", title: "Whole-mouth findings", kind: "statements", parts: [] },
  ],
});

const item = (tooth: string, finding: string, extra: Record<string, unknown> = {}) => ({
  tooth,
  text: `${tooth} ${finding}`,
  finding,
  detail: null,
  grade: null,
  mm: null,
  planned: false,
  evidence: [2],
  ...extra,
});

const run = (chart: unknown[], faithfulness: Partial<NoteRun["faithfulness"]> = {}): NoteRun => ({
  status: "draft",
  template: "dental@0.2.0",
  model: "gemma-4-E4B_q4_0-it",
  note: { dental_chart: chart, whole_mouth_findings: [] },
  faithfulness: {
    passed: true,
    statements_checked: chart.length,
    numbers_checked: 0,
    unsupported_numbers: [],
    citation_mismatches: [],
    invalid_evidence: [],
    ...faithfulness,
  },
  stats: { prompt_tokens: 1, completion_tokens: 1, total_ms: 1, tokens_per_second: 1, time_to_first_token_ms: 1 },
});

const chart = [
  item("108", "fracture", { detail: "complicated_crown", evidence: [4] }),
  item("108", "extraction", { detail: "open", evidence: [8] }),
  item("209", "missing", { evidence: [5] }),
];

describe("NoteView with a tooth chart", () => {
  it("draws the chart, lists findings by tooth with codes, and keeps blank teeth blank", () => {
    const { container } = render(
      <NoteView template={template("canine")} run={run(chart)} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />,
    );
    expect(container.querySelectorAll("[data-tooth]").length).toBeGreaterThanOrEqual(42);
    expect(container.querySelector('g[data-tooth="108"]')?.hasAttribute("data-charted")).toBe(true);
    expect(container.querySelector('g[data-tooth="110"]')?.hasAttribute("data-charted")).toBe(false);
    expect(screen.getByText(/Complicated crown fracture \(T\/FX\/CCF\)/)).toBeTruthy();
    expect(screen.getByText(/Blank: not charted in this recording/)).toBeTruthy();
  });

  it("opens the chart editor on a tooth in a draft, and only lights the lines once signed", () => {
    const onEdit = vi.fn();
    const onEvidence = vi.fn();
    const { container, rerender } = render(
      <NoteView template={template("canine")} run={run(chart)} onEvidence={onEvidence} onPlay={() => {}} onCopySection={() => {}} onEdit={onEdit} />,
    );
    fireEvent.click(container.querySelector('g[data-tooth="209"]') as Element);
    expect(onEdit).toHaveBeenCalledWith("dental_chart", { tooth: "209" });
    // Signed: no onEdit; a click selects the tooth and lights its lines.
    rerender(<NoteView template={template("canine")} run={run(chart)} onEvidence={onEvidence} onPlay={() => {}} onCopySection={() => {}} />);
    fireEvent.click(container.querySelector('g[data-tooth="108"]') as Element);
    expect(onEvidence).toHaveBeenLastCalledWith([4, 8]);
    expect(container.querySelector('g[data-tooth="108"]')?.hasAttribute("data-selected")).toBe(true);
  });

  it("shows a hard fail for a tooth said nowhere and a name conflict as a soft flag", () => {
    render(
      <NoteView
        template={template("canine")}
        run={run(chart, {
          passed: false,
          unsupported_numbers: [{ path: "/dental_chart/2", number: "209", cited: [5], found_in: [] }],
          tooth_flags: [{ path: "/dental_chart/0", tooth: "108", kind: "name_conflict", other: "208" }],
        })}
        onEvidence={() => {}}
        onPlay={() => {}}
        onCopySection={() => {}}
      />,
    );
    expect(screen.getByText(/209 isn't anywhere in the transcript/)).toBeTruthy();
    const conflict = screen.getByTitle(/Said as 108 and as left maxillary fourth premolar \(208\)/);
    expect(conflict.getAttribute("data-soft")).toBe("true");
  });

  it("lists findings without a diagram when the dentition wasn't recorded, and says so", () => {
    const { container } = render(
      <NoteView template={template(null)} run={run(chart)} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />,
    );
    expect(container.querySelector("svg.dc-svg")).toBeNull();
    expect(screen.getByText(/Dentition not recorded/)).toBeTruthy();
  });

  it("copies a chart heading as words, never the diagram's tooth numbers", () => {
    const onCopySection = vi.fn();
    render(<NoteView template={template("feline")} run={run(chart)} onEvidence={() => {}} onPlay={() => {}} onCopySection={onCopySection} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy Dental chart" }));
    expect(onCopySection).toHaveBeenCalledWith(
      "Dental chart",
      "108: complicated crown fracture (T/FX/CCF); open extraction (XSS)\n209: missing",
    );
  });

  it("puts the chart in Copy note as one line per tooth, with citations when asked", () => {
    const out = formatNote(template("canine"), run(chart), { format: "plain", citations: true, header: [], addenda: [] });
    expect(out.text).toContain("108: complicated crown fracture (T/FX/CCF) [4]; open extraction (XSS) [8]");
    expect(out.text).toContain("209: missing [5]");
  });
});
