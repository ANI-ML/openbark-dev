import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Toast } from "./Toast";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Toast", () => {
  it("offers its action once, then closes", () => {
    const run = vi.fn();
    const onClose = vi.fn();
    render(<Toast message="Visit deleted" action={{ label: "Undo", run }} onClose={onClose} />);
    expect(screen.getByRole("status").textContent).toContain("Visit deleted");
    const undo = screen.getByRole("button", { name: "Undo" });
    fireEvent.click(undo);
    fireEvent.click(undo);
    expect(run).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalled();
  });

  it("closes after 4 seconds", () => {
    const onClose = vi.fn();
    render(<Toast message="Note signed" onClose={onClose} />);
    act(() => vi.advanceTimersByTime(3900));
    expect(onClose).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(200));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("stays open while it has keyboard focus or the pointer", () => {
    const onClose = vi.fn();
    render(<Toast message="Visit deleted" action={{ label: "Undo", run: () => {} }} onClose={onClose} />);
    const undo = screen.getByRole("button", { name: "Undo" });
    act(() => undo.focus());
    act(() => vi.advanceTimersByTime(10_000));
    expect(onClose).not.toHaveBeenCalled();
    act(() => undo.blur());
    fireEvent.pointerEnter(screen.getByRole("status"));
    act(() => vi.advanceTimersByTime(10_000));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.pointerLeave(screen.getByRole("status"));
    act(() => vi.advanceTimersByTime(4100));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("carries on from the time left after a hold, and shows it on a bar that holds too", () => {
    const onClose = vi.fn();
    render(<Toast message="Visit deleted" action={{ label: "Undo", run: () => {} }} onClose={onClose} />);
    const toast = screen.getByRole("status");
    const bar = toast.querySelector(".ob-toast-time") as HTMLElement;
    expect(bar.style.animationDuration).toBe("4000ms");
    expect(bar.style.animationPlayState).toBe("running");
    act(() => vi.advanceTimersByTime(3000));
    fireEvent.pointerEnter(toast);
    expect(bar.style.animationPlayState).toBe("paused");
    act(() => vi.advanceTimersByTime(10_000));
    fireEvent.pointerLeave(toast);
    // About a second was left when the pointer arrived; the clock doesn't start over.
    act(() => vi.advanceTimersByTime(1100));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
