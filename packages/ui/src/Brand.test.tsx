import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Lockup, Mark, MarkMono, Wordmark } from "./Brand";
import { Button } from "./Button";
import { StatusDot } from "./StatusDot";

afterEach(cleanup);

describe("Mark", () => {
  it("draws the paw mic from the brand geometry, decorative by default", () => {
    const { container } = render(<Mark size={48} />);
    const svg = container.querySelector("svg.ob-mark") as SVGElement;
    expect(svg.getAttribute("viewBox")).toBe("0 0 100 100");
    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.querySelectorAll("circle.ob-mark-fg")).toHaveLength(3);
    const capsule = svg.querySelector(".ob-mark-capsule") as SVGRectElement;
    expect([capsule.getAttribute("x"), capsule.getAttribute("y"), capsule.getAttribute("rx")]).toEqual(["35", "28", "15"]);
    expect(svg.querySelectorAll(".ob-mark-grille")).toHaveLength(2);
    expect(svg.querySelector(".ob-mark-cradle")?.getAttribute("d")).toBe("M24 54 A26 26 0 0 0 76 54");
  });

  it("drops the grille below 24 px", () => {
    const { container } = render(<Mark size={16} />);
    expect(container.querySelectorAll(".ob-mark-grille")).toHaveLength(0);
  });

  it("takes a name when it is the only label, and a forced tone for tiles", () => {
    render(<Mark title="OpenBark" tone="dark" />);
    const svg = screen.getByRole("img", { name: "OpenBark" });
    expect(svg.getAttribute("data-tone")).toBe("dark");
  });
});

describe("MarkMono", () => {
  it("uses currentColor and cuts the grille with a per-instance mask", () => {
    const { container } = render(
      <>
        <MarkMono size={48} />
        <MarkMono size={48} />
      </>,
    );
    const masks = Array.from(container.querySelectorAll("mask")).map((m) => m.id);
    expect(masks).toHaveLength(2);
    expect(new Set(masks).size).toBe(2);
    expect(masks[0]).toMatch(/^[a-zA-Z0-9_-]+$/);
    expect(container.querySelector("g")?.getAttribute("fill")).toBe("currentColor");
  });
});

describe("Wordmark and Lockup", () => {
  it("is lowercase openbark with the 500/800 split, named OpenBark", () => {
    render(<Wordmark />);
    const word = screen.getByRole("img", { name: "OpenBark" });
    expect(word.textContent).toBe("openbark");
    expect(word.querySelector(".ob-wordmark-open")?.textContent).toBe("open");
    expect(word.querySelector(".ob-wordmark-bark")?.textContent).toBe("bark");
  });

  it("keeps the lockup's proportions at any height", () => {
    render(<Lockup size={40} />);
    const svg = screen.getByRole("img", { name: "OpenBark" });
    expect(svg.getAttribute("viewBox")).toBe("0 0 372 80");
    expect(svg.getAttribute("width")).toBe("186");
  });
});

describe("Button and StatusDot", () => {
  it("has the record variant and the three heights", () => {
    render(
      <>
        <Button variant="record">Record</Button>
        <Button size="compact">Compact</Button>
        <Button>Default</Button>
        <Button variant="primary" size="prominent">
          Prominent
        </Button>
      </>,
    );
    expect(screen.getByText("Record").getAttribute("data-variant")).toBe("record");
    expect(screen.getByText("Compact").getAttribute("data-size")).toBe("compact");
    expect(screen.getByText("Default").hasAttribute("data-size")).toBe(false);
    expect(screen.getByText("Prominent").getAttribute("data-size")).toBe("prominent");
  });

  it("pairs the dot with text and can pulse while recording", () => {
    const { container } = render(
      <StatusDot tone="record" live>
        Recording
      </StatusDot>,
    );
    expect(screen.getByText("Recording")).toBeTruthy();
    expect(container.querySelector(".ob-dot")?.hasAttribute("data-live")).toBe(true);
  });
});
