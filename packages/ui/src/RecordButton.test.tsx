import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { RecordButton } from "./RecordButton";
import { createLevelSource } from "./Waveform";

afterEach(cleanup);

it("is a labelled toggle that reflects the recording state", async () => {
  const onToggle = vi.fn();
  const levels = createLevelSource();
  const { rerender } = render(<RecordButton recording={false} onToggle={onToggle} levels={levels} />);
  const idle = screen.getByRole("button", { name: "Start recording" });
  expect(idle.getAttribute("aria-pressed")).toBe("false");
  await userEvent.click(idle);
  expect(onToggle).toHaveBeenCalledOnce();

  rerender(<RecordButton recording onToggle={onToggle} levels={levels} />);
  const rec = screen.getByRole("button", { name: "Stop recording" });
  expect(rec.getAttribute("aria-pressed")).toBe("true");
  // The live waveform reacts to levels without React re-rendering.
  levels.push(1);
  const bar = rec.querySelector(".ob-wave span:last-child") as HTMLElement;
  expect(bar.style.transform).toBe("scaleY(1)");
});

it("can't be pressed while starting or stopping", () => {
  render(<RecordButton recording={false} busy onToggle={() => {}} levels={createLevelSource()} />);
  expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
});
