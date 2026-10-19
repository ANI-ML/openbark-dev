import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Skeleton } from "./Skeleton";

afterEach(cleanup);

describe("Skeleton", () => {
  it("draws list rows under one named status, the shapes hidden from screen readers", () => {
    render(<Skeleton rows={3} label="Loading" />);
    const status = screen.getByRole("status", { name: "Loading" });
    const rows = status.querySelectorAll(".ob-skeleton-row");
    expect(rows).toHaveLength(3);
    for (const row of rows) expect(row.getAttribute("aria-hidden")).toBe("true");
  });

  it("draws lines of text with ragged widths", () => {
    render(<Skeleton lines={4} />);
    const lines = screen.getByRole("status").querySelectorAll(".ob-skeleton");
    expect(lines).toHaveLength(4);
    expect(new Set([...lines].map((l) => (l as HTMLElement).style.width)).size).toBeGreaterThan(1);
  });

  it("falls back to one block of the given height", () => {
    render(<Skeleton height={72} />);
    expect(screen.getByRole("status").style.height).toBe("72px");
  });
});
