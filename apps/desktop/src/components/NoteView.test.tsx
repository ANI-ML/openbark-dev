import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NoteRun, Template } from "../engine";
import { NoteView } from "./NoteView";

afterEach(cleanup);

const template: Template = {
  id: "soap",
  version: "0.1.0",
  name: "SOAP note",
  sections: [
    { id: "patient", title: "Patient", kind: "fields", fields: [{ key: "name", label: "Name" }, { key: "species", label: "Species" }], parts: [] },
    {
      id: "objective",
      title: "Objective",
      kind: null,
      fields: [],
      parts: [
        { key: "vitals", label: "Vitals", kind: "fields", presence: "if_discussed", fields: [{ key: "temperature", label: "Temperature" }, { key: "heart_rate", label: "HR" }] },
        { key: "physical_exam", label: "Physical exam", kind: "statements", presence: "if_discussed", fields: [] },
      ],
    },
  ],
};

const run = (overrides: Partial<NoteRun["faithfulness"]> = {}): NoteRun => ({
  status: "draft",
  template: "soap@0.1.0",
  model: "gemma-4-E4B_q4_0-it",
  note: {
    patient: { name: { value: "Maple", evidence: [0] }, species: null },
    objective: {
      vitals: { temperature: { value: "38.6 °C", evidence: [2] }, heart_rate: { value: "240 bpm", evidence: [2] } },
      physical_exam: [],
    },
  },
  faithfulness: {
    passed: true,
    statements_checked: 3,
    numbers_checked: 2,
    unsupported_numbers: [],
    citation_mismatches: [],
    invalid_evidence: [],
    ...overrides,
  },
  stats: { prompt_tokens: 1, completion_tokens: 1, total_ms: 1, tokens_per_second: 1, time_to_first_token_ms: 1 },
});

describe("NoteView", () => {
  it("renders fields, lists what wasn't discussed and never omits empty parts", () => {
    render(<NoteView template={template} run={run()} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    expect(screen.getByText("Maple")).toBeTruthy();
    expect(screen.getByText("Species")).toBeTruthy(); // listed under "Not discussed"
    expect(screen.getAllByText("Not discussed").length).toBeGreaterThanOrEqual(2);
  });

  it("lights the cited transcript lines on hover and plays them on click", () => {
    const onEvidence = vi.fn();
    const onPlay = vi.fn();
    render(<NoteView template={template} run={run()} onEvidence={onEvidence} onPlay={onPlay} onCopySection={() => {}} />);
    const temp = screen.getByText("38.6 °C").closest(".stmt")!;
    fireEvent.mouseEnter(temp);
    expect(onEvidence).toHaveBeenLastCalledWith([2]);
    fireEvent.click(temp);
    expect(onPlay).toHaveBeenCalledWith([2]);
    fireEvent.mouseLeave(temp);
    expect(onEvidence).toHaveBeenLastCalledWith(null);
  });

  it("wraps a number absent from the transcript in a hard-fail callout", () => {
    const r = run({ passed: false, unsupported_numbers: [{ path: "/objective/vitals/heart_rate", number: "240", cited: [2], found_in: [] }] });
    const { container } = render(<NoteView template={template} run={r} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    const callout = container.querySelector(".hardfail")!;
    expect(callout.textContent).toContain("240 isn't anywhere in the transcript");
  });

  it("underlines a value cited to the wrong line", () => {
    const r = run({ citation_mismatches: [{ path: "/objective/vitals/temperature", number: "38.6", cited: [2], found_in: [5] }] });
    render(<NoteView template={template} run={r} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    const stmt = screen.getByText("38.6 °C").closest(".stmt")!;
    expect(stmt.getAttribute("data-soft")).toBe("true");
    expect(stmt.getAttribute("title")).toContain("not in the cited line");
  });
  it("renders content held directly under a heading (team templates)", () => {
    const team: Template = {
      id: "dental",
      version: "1",
      name: "Dental",
      sections: [
        { id: "anaesthesia", title: "Anaesthesia", kind: "medications" },
        { id: "findings", title: "Findings", kind: "statements" },
      ],
    };
    const r = { ...run(), note: { anaesthesia: [{ drug: "Propofol", dose: "4 mg/kg", route: "IV", frequency: null, duration: null, evidence: [1] }], findings: [] } };
    render(<NoteView template={team} run={r} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    expect(screen.getByText("Propofol")).toBeTruthy();
    expect(screen.getByText(/4 mg\/kg, IV/)).toBeTruthy();
    expect(screen.getByText("Not discussed")).toBeTruthy();
  });

  it("asks about a heading or subheading and shows the panel beneath it", () => {
    const onAsk = vi.fn();
    const { rerender } = render(
      <NoteView template={template} run={run()} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} onAsk={onAsk} renderAsk={() => null} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Ask for changes to Vitals" }));
    expect(onAsk).toHaveBeenLastCalledWith("objective/vitals");
    fireEvent.click(screen.getByRole("button", { name: "Ask for changes to Objective" }));
    expect(onAsk).toHaveBeenLastCalledWith("objective");
    rerender(
      <NoteView
        template={template}
        run={run()}
        onEvidence={() => {}}
        onPlay={() => {}}
        onCopySection={() => {}}
        onAsk={onAsk}
        renderAsk={(h) => (h === "objective/vitals" ? <p>panel for vitals</p> : null)}
      />,
    );
    expect(screen.getByText("panel for vitals")).toBeTruthy();
  });

  it("has no Ask actions without onAsk (signed notes) and marks a redrafted heading", () => {
    const r = { ...run(), refinement: { heading: "objective/vitals", request: "Add the weight", saved_as_standard: true } };
    render(<NoteView template={template} run={r} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    expect(screen.queryByRole("button", { name: /Ask for changes/ })).toBeNull();
    expect(screen.getByText(/Redrafted on request/).textContent).toContain("Add the weight");
    expect(screen.getByText(/Saved as a standard/)).toBeTruthy();
  });
  it("flags a line whose words aren't in its cited line, with the likely line", () => {
    const r = run({ unsupported_statements: [{ path: "/objective/vitals/temperature", missing: ["fever"], cited: [2], suggested: [5] }] });
    render(<NoteView template={template} run={r} onEvidence={() => {}} onPlay={() => {}} onCopySection={() => {}} />);
    const stmt = screen.getByText("38.6 °C").closest(".stmt")!;
    expect(stmt.getAttribute("data-soft")).toBe("true");
    expect(stmt.getAttribute("title")).toContain("(fever)");
    expect(stmt.getAttribute("title")).toContain("It may come from line 5.");
  });
});
