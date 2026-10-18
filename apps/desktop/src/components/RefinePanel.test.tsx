import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RefinePanel } from "./RefinePanel";

afterEach(cleanup);

const standard = {
  id: "s1",
  heading: "plan/medications",
  text: "List every fluid with its volume and rate",
  added_at: "2026-10-17T20:00:00Z",
};

const panel = (onSubmit = vi.fn(), standards = [standard]) =>
  render(
    <RefinePanel
      title="Plan > Medications"
      templateName="SOAP note"
      standards={standards}
      canSave
      busy={false}
      board={null}
      onSubmit={onSubmit}
      onCancel={() => {}}
    />,
  );

describe("RefinePanel", () => {
  it("sends the request and whether to save it as a standard", () => {
    const onSubmit = vi.fn();
    panel(onSubmit, []);
    const button = screen.getByRole("button", { name: "Redraft Medications" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("What should change under Plan > Medications?"), {
      target: { value: "Include the fluids" },
    });
    fireEvent.click(screen.getByLabelText(/Save as a standard/));
    fireEvent.click(button);
    expect(onSubmit).toHaveBeenCalledWith("Include the fluids", true);
  });

  it("offers saved standards as reusable requests without saving them twice", () => {
    const onSubmit = vi.fn();
    panel(onSubmit);
    fireEvent.click(screen.getByRole("button", { name: /List every fluid/ }));
    expect(screen.getByText("Already a standard in SOAP note.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Redraft Medications" }));
    expect(onSubmit).toHaveBeenCalledWith(standard.text, false);
  });
});
