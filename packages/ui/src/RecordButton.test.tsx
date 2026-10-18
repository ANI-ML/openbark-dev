import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RecordButton, formatElapsed } from "./RecordButton";
import { Waveform, createLevelSource } from "./Waveform";

afterEach(cleanup);

it("is a labelled toggle that reflects the recording state", async () => {
  const onToggle = vi.fn();
  const levels = createLevelSource();
  const { rerender } = render(<RecordButton recording={false} onToggle={onToggle} levels={levels} />);
  const idle = screen.getByRole("button", { name: "Start recording" });
  expect(idle.getAttribute("aria-pressed")).toBe("false");
  expect(idle.getAttribute("data-state")).toBe("idle");
  await userEvent.click(idle);
  expect(onToggle).toHaveBeenCalledOnce();

  rerender(<RecordButton recording onToggle={onToggle} levels={levels} />);
  const rec = screen.getByRole("button", { name: "Stop recording" });
  expect(rec.getAttribute("aria-pressed")).toBe("true");
  expect(rec.getAttribute("data-state")).toBe("recording");
});

it("can't be pressed while starting or stopping", () => {
  render(<RecordButton recording={false} busy onToggle={() => {}} levels={createLevelSource()} />);
  expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
});

describe("labels (§7.1)", () => {
  it("says Record when idle, with the white mono mark as its glyph", () => {
    const { container } = render(<RecordButton recording={false} onToggle={() => {}} />);
    expect(screen.getByText("Record")).toBeTruthy();
    expect(container.querySelector(".ob-record .ob-mark-mono")).not.toBeNull();
    expect(container.querySelector(".ob-record-pulse")).toBeNull();
  });

  it("shows the elapsed time while recording, with the pulse ring", () => {
    const { container } = render(<RecordButton recording elapsed={134} onToggle={() => {}} />);
    expect(screen.getByText("Recording · 02:14")).toBeTruthy();
    expect(container.querySelector(".ob-record-pulse")).not.toBeNull();
  });

  it("writes the note with an indeterminate ring and can't be pressed", () => {
    const { container } = render(<RecordButton recording={false} processing onToggle={() => {}} />);
    const button = screen.getByRole("button", { name: "Writing note…" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(container.querySelector(".ob-record-spinner")).not.toBeNull();
    expect(container.querySelector(".ob-record-label")?.textContent).toBe("Writing note…");
  });

  it("uses translated labels", () => {
    render(
      <RecordButton
        recording
        elapsed={5}
        onToggle={() => {}}
        labels={{ start: "Aufnahme starten", stop: "Aufnahme beenden", recording: "Aufnahme" }}
      />,
    );
    expect(screen.getByRole("button", { name: "Aufnahme beenden" })).toBeTruthy();
    expect(screen.getByText("Aufnahme · 00:05")).toBeTruthy();
  });

  it("can hide its visible label when the screen shows its own", () => {
    const { container } = render(<RecordButton recording={false} hideLabel onToggle={() => {}} />);
    expect(container.querySelector(".ob-record-label")).toBeNull();
  });
});

it("announces the state politely, but not every second of the timer", () => {
  const { container, rerender } = render(<RecordButton recording={false} onToggle={() => {}} />);
  const live = container.querySelector("[aria-live='polite']") as HTMLElement;
  expect(live.textContent).toBe("");
  rerender(<RecordButton recording elapsed={1} onToggle={() => {}} />);
  expect(live.textContent).toBe("Recording");
  rerender(<RecordButton recording elapsed={2} onToggle={() => {}} />);
  expect(live.textContent).toBe("Recording");
  rerender(<RecordButton recording={false} processing onToggle={() => {}} />);
  expect(live.textContent).toBe("Writing note…");
});

it("formats elapsed time as mm:ss, and h:mm:ss past an hour", () => {
  expect(formatElapsed(0)).toBe("00:00");
  expect(formatElapsed(134.9)).toBe("02:14");
  expect(formatElapsed(3725)).toBe("1:02:05");
});

describe("Waveform (§3.5)", () => {
  it("reacts to levels without React re-rendering", () => {
    const levels = createLevelSource();
    const { container } = render(<Waveform levels={levels} bars={9} />);
    levels.push(1);
    const bar = container.querySelector(".ob-wave span:last-child") as HTMLElement;
    expect(bar.style.transform).toBe("scaleY(1)");
  });

  it("marks the current bar for the accent only while live", () => {
    const levels = createLevelSource();
    const { container, rerender } = render(<Waveform levels={levels} />);
    expect(container.querySelector(".ob-wave")?.hasAttribute("data-active")).toBe(true);
    rerender(<Waveform levels={levels} active={false} />);
    expect(container.querySelector(".ob-wave")?.hasAttribute("data-active")).toBe(false);
  });
});
