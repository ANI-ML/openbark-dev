import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HeadingEditor } from "./HeadingEditor";

afterEach(cleanup);

describe("HeadingEditor", () => {
  it("keeps each line's citation, drops emptied lines and adds new ones without evidence", () => {
    const onSave = vi.fn();
    render(
      <HeadingEditor
        title="Plan > Follow-up"
        kind="statements"
        value={[
          { text: "Recheck Sunday", evidence: [41] },
          { text: "Call with results", evidence: [41] },
        ]}
        busy={false}
        onSave={onSave}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("Line 1"), { target: { value: "Recheck Sunday morning" } });
    fireEvent.change(screen.getByLabelText("Line 2"), { target: { value: "  " } });
    fireEvent.click(screen.getByRole("button", { name: "Add line" }));
    fireEvent.change(screen.getByLabelText("Line 3"), { target: { value: "Owner to bring a stool sample" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(onSave).toHaveBeenCalledWith([
      { text: "Recheck Sunday morning", evidence: [41] },
      { text: "Owner to bring a stool sample", evidence: [] },
    ]);
  });

  it("edits measurements with units and clears one back to not discussed", () => {
    const onSave = vi.fn();
    render(
      <HeadingEditor
        title="Vitals"
        kind="fields"
        fields={[
          { key: "weight", label: "Weight" },
          { key: "heart_rate", label: "HR" },
        ]}
        value={{ weight: { value: 4.1, unit: "kg", evidence: [18] }, heart_rate: { value: 200, unit: "bpm", evidence: [20] } }}
        busy={false}
        onSave={onSave}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("Weight"), { target: { value: "4.3" } });
    fireEvent.change(screen.getByLabelText("HR"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(onSave).toHaveBeenCalledWith({ weight: { value: "4.3", unit: "kg", evidence: [18] }, heart_rate: null });
  });
});
