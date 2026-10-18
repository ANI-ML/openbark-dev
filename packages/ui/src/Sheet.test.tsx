import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Dialog, Sheet } from "./Sheet";
import { project, rubberband } from "./spring";

describe("Sheet", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("plays its exit before closing, and Escape closes it", () => {
    const onClose = vi.fn();
    render(
      <Sheet label="Discharge" onClose={onClose}>
        {(close) => (
          <button type="button" onClick={close}>
            Done
          </button>
        )}
      </Sheet>,
    );
    fireEvent.click(screen.getByText("Done"));
    expect(screen.getByRole("dialog").hasAttribute("data-closing")).toBe(true);
    expect(onClose).not.toHaveBeenCalled();
    act(() => void vi.advanceTimersByTime(500));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("can't be dismissed while work is running", () => {
    const onClose = vi.fn();
    render(
      <Sheet label="Busy" onClose={onClose} dismissible={false}>
        <p>Working</p>
      </Sheet>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    act(() => void vi.advanceTimersByTime(500));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("a notice that needs an answer ignores Escape until answered", () => {
    const onClose = vi.fn();
    render(
      <Dialog labelledBy="t" onClose={onClose} dismissible={false}>
        {(close) => (
          <>
            <h2 id="t">Before you record</h2>
            <button type="button" onClick={close}>
              Accept
            </button>
          </>
        )}
      </Dialog>,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    act(() => void vi.advanceTimersByTime(500));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Accept"));
    act(() => void vi.advanceTimersByTime(500));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("gesture maths", () => {
  it("projects a flick further than where it was released", () => {
    expect(project(1000)).toBeGreaterThan(400);
    expect(project(0)).toBe(0);
  });

  it("resists more the further it is pulled past the edge", () => {
    const a = rubberband(-50, 600);
    const b = rubberband(-200, 600);
    expect(Math.abs(a)).toBeLessThan(50);
    expect(Math.abs(b) - Math.abs(a)).toBeLessThan(150);
  });
});
