import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Dentition, ToothFinding } from "../dental";
import { DentalChartEditor } from "./DentalChartEditor";

afterEach(cleanup);

const f = (tooth: string, finding: string, extra: Partial<ToothFinding> = {}): ToothFinding => ({
  tooth,
  text: "as said",
  finding,
  detail: null,
  grade: null,
  mm: null,
  planned: false,
  evidence: [1],
  ...extra,
});

function setup(value: ToothFinding[] = [], dentition: Dentition | null = "canine", initialSelection?: string[]) {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  const onPlay = vi.fn();
  const view = render(
    <DentalChartEditor
      title="Dental chart"
      dentition={dentition}
      value={value}
      busy={false}
      initialSelection={initialSelection}
      onSave={onSave}
      onCancel={onCancel}
      onPlay={onPlay}
      lineText={(id) => (id === 4 ? "Two oh eight has a slab fracture with pulp exposure." : undefined)}
    />,
  );
  const tooth = (id: string) => view.container.querySelector<SVGGElement>(`g.dc-tooth[data-tooth="${id}"]`)!;
  const save = () => fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  return { ...view, onSave, onCancel, onPlay, tooth, save };
}

const choose = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });

describe("DentalChartEditor", () => {
  it("adds a finding to the clicked tooth as the vet's own entry", () => {
    const { tooth, save, onSave } = setup([f("108", "missing")]);
    fireEvent.click(tooth("309"));
    expect(screen.getByText("309, left mandibular first molar")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add finding" }));
    choose("What was found", "mobility");
    choose("Grade", "2");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    save();
    expect(onSave).toHaveBeenCalledWith(
      [
        f("108", "missing"),
        { tooth: "309", text: "Mobility 2", finding: "mobility", detail: null, grade: 2, mm: null, planned: false, evidence: [] },
      ],
      null,
    );
  });

  it("marks several teeth missing at once with Ctrl or Cmd click", () => {
    const { tooth, save, onSave } = setup();
    fireEvent.click(tooth("101"));
    fireEvent.click(tooth("102"), { ctrlKey: true });
    fireEvent.click(tooth("103"), { metaKey: true });
    expect(screen.getByText("3 teeth selected: 101, 102, 103")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Mark missing" }));
    save();
    const saved = onSave.mock.calls[0]![0] as ToothFinding[];
    expect(saved.map((i) => [i.tooth, i.finding, i.text, i.evidence])).toEqual([
      ["101", "missing", "Missing", []],
      ["102", "missing", "Missing", []],
      ["103", "missing", "Missing", []],
    ]);
  });

  it("changes a finding from the recording in place, keeping its link, and leaves the others alone", () => {
    const fracture = f("208", "fracture", { evidence: [4], text: "slab fracture" });
    const extraction = f("208", "extraction", { detail: "open", evidence: [11] });
    const { tooth, save, onSave } = setup([fracture, extraction]);
    fireEvent.click(tooth("208"));
    expect(screen.getByText("“Two oh eight has a slab fracture with pulp exposure.”")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Change fracture on 208" }));
    expect((screen.getByLabelText("What was found") as HTMLSelectElement).value).toBe("fracture");
    choose("Type", "complicated_crown");
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    save();
    const saved = onSave.mock.calls[0]![0] as ToothFinding[];
    expect(saved[0]).toEqual({ ...fracture, detail: "complicated_crown", text: "Complicated crown fracture", evidence: [4] });
    expect(saved[1]).toEqual(extraction);
  });

  it("removes a finding", () => {
    const { tooth, save, onSave } = setup([f("208", "fracture"), f("208", "extraction", { detail: "open" })]);
    fireEvent.click(tooth("208"));
    fireEvent.click(screen.getByRole("button", { name: "Remove open extraction from 208" }));
    save();
    expect(onSave.mock.calls[0]![0]).toEqual([f("208", "fracture")]);
  });

  it("plays a finding's audio from its row", () => {
    const { tooth, onPlay } = setup([f("208", "fracture", { evidence: [4] })]);
    fireEvent.click(tooth("208"));
    fireEvent.click(screen.getByRole("button", { name: "Play the supporting audio" }));
    expect(onPlay).toHaveBeenCalledWith([4]);
  });

  it("offers only the controls a finding uses", () => {
    const { tooth } = setup();
    fireEvent.click(tooth("309"));
    fireEvent.click(screen.getByRole("button", { name: "Add finding" }));
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
    choose("What was found", "mobility");
    expect(screen.queryByLabelText("Millimetres")).toBeNull();
    expect(screen.queryByLabelText("Type")).toBeNull();
    expect([...(screen.getByLabelText("Grade") as HTMLSelectElement).options].map((o) => o.value)).toEqual(["", "0", "1", "2", "3"]);
    choose("What was found", "probing_depth");
    expect(screen.queryByLabelText("Grade")).toBeNull();
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(true);
    choose("Millimetres", "6.5");
    expect((screen.getByRole("button", { name: "Add" }) as HTMLButtonElement).disabled).toBe(false);
    choose("What was found", "extraction");
    expect([...(screen.getByLabelText("Technique") as HTMLSelectElement).options].map((o) => o.value)).toEqual([
      "",
      "closed",
      "closed_sectioned",
      "open",
    ]);
    expect(screen.getByLabelText("Planned, not done yet")).toBeTruthy();
    choose("What was found", "fracture");
    expect(screen.queryByLabelText("Planned, not done yet")).toBeNull();
    expect((screen.getByLabelText("Type") as HTMLSelectElement).options).toHaveLength(8);
  });

  it("records a resorption sub-stage and a planned extraction", () => {
    const { tooth, save, onSave } = setup();
    fireEvent.click(tooth("307"));
    fireEvent.click(screen.getByRole("button", { name: "Add finding" }));
    choose("What was found", "resorption");
    choose("Stage", "4b");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("button", { name: "Add finding" }));
    choose("What was found", "extraction");
    fireEvent.click(screen.getByLabelText("Planned, not done yet"));
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    save();
    const saved = onSave.mock.calls[0]![0] as ToothFinding[];
    expect(saved.map((i) => [i.finding, i.grade, i.detail, i.planned, i.text])).toEqual([
      ["resorption", 4, "crown_worse", false, "Tooth resorption stage 4b"],
      ["extraction", null, null, true, "Planned: extraction"],
    ]);
  });

  it("works by keyboard alone", () => {
    const { container, save, onSave } = setup();
    const stop = container.querySelector<SVGGElement>('g.dc-tooth[tabindex="0"]')!;
    expect(stop.getAttribute("data-tooth")).toBe("110");
    stop.focus();
    for (let i = 0; i < 17; i++) fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("208");
    fireEvent.keyDown(document.activeElement!, { key: " " });
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    fireEvent.keyDown(document.activeElement!, { key: " " });
    expect(screen.getByText("2 teeth selected: 208, 209")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Mark missing" }));
    save();
    expect((onSave.mock.calls[0]![0] as ToothFinding[]).map((i) => i.tooth)).toEqual(["208", "209"]);
  });

  it("closes the form on Escape, then clears the selection", () => {
    const { tooth } = setup();
    fireEvent.click(tooth("309"));
    fireEvent.click(screen.getByRole("button", { name: "Add finding" }));
    expect(screen.getByLabelText("What was found")).toBeTruthy();
    fireEvent.keyDown(screen.getByLabelText("What was found"), { key: "Escape" });
    expect(screen.queryByLabelText("What was found")).toBeNull();
    expect(screen.getByText("309, left mandibular first molar")).toBeTruthy();
    fireEvent.keyDown(tooth("309"), { key: "Escape" });
    expect(screen.getByText("Select teeth on the chart to add findings.")).toBeTruthy();
  });

  it("only marks teeth that have no findings yet", () => {
    const { tooth } = setup([f("309", "mobility", { grade: 2 })], "canine", ["309"]);
    expect(tooth("309").hasAttribute("data-selected")).toBe(true);
    const missing = screen.getByRole("button", { name: "Mark missing" }) as HTMLButtonElement;
    expect(missing.disabled).toBe(true);
    expect(screen.getByText("Mark missing, Mark normal: only for teeth with no findings yet.")).toBeTruthy();
    fireEvent.click(tooth("310"));
    expect(missing.disabled).toBe(false);
  });

  it("asks dog or cat before saving a chart with no dentition, and refuses a cat's missing teeth", () => {
    const { save, onSave } = setup([f("105", "missing")], null);
    const saveButton = screen.getByRole("button", { name: "Save changes" }) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);
    expect(screen.getByText("Choose dog or cat before saving.")).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "Cat" }));
    expect(screen.getByRole("alert").textContent).toBe("105 aren't in a cat's mouth. Remove them or choose Dog.");
    expect(saveButton.disabled).toBe(true);
    fireEvent.click(screen.getByRole("radio", { name: "Dog" }));
    expect(saveButton.disabled).toBe(false);
    save();
    expect(onSave).toHaveBeenCalledWith([f("105", "missing")], "canine");
  });

  it("draws a cat's teeth once Cat is chosen", () => {
    const { container } = setup([], null);
    expect(container.querySelectorAll("g.dc-tooth")).toHaveLength(42);
    fireEvent.click(screen.getByRole("radio", { name: "Cat" }));
    expect(container.querySelectorAll("g.dc-tooth")).toHaveLength(30);
    fireEvent.click(screen.getByLabelText("Deciduous teeth"));
    expect(container.querySelectorAll("g.dc-tooth")).toHaveLength(56);
  });

  it("sends nothing on Cancel", () => {
    const { tooth, onSave, onCancel } = setup();
    fireEvent.click(tooth("101"));
    fireEvent.click(screen.getByRole("button", { name: "Mark missing" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
    expect(onSave).not.toHaveBeenCalled();
  });
});
