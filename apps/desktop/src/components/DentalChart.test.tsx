import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChartFaithfulness, ToothFinding } from "../dental";
import { DentalChart } from "./DentalChart";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

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

const clean: ChartFaithfulness = {
  passed: true,
  statements_checked: 0,
  numbers_checked: 0,
  unsupported_numbers: [],
  citation_mismatches: [],
  invalid_evidence: [],
};

/** A dog's dental: a range, a name-only tooth, a plan and periodontal findings. */
const juno: ToothFinding[] = [
  f("101", "missing", { evidence: [2] }),
  f("102", "missing", { evidence: [2] }),
  f("103", "missing", { evidence: [2] }),
  f("104", "normal", { evidence: [3] }),
  f("208", "fracture", { detail: "complicated_crown", evidence: [4] }),
  f("208", "extraction", { detail: "open", evidence: [11] }),
  f("304", "extraction", { planned: true, evidence: [13] }),
  f("309", "periodontal_stage", { grade: 3, evidence: [7] }),
  f("309", "mobility", { grade: 2, evidence: [7, 8] }),
  f("209", "missing", { evidence: [5] }),
];

const toothEls = (c: HTMLElement) => [...c.querySelectorAll<SVGGElement>("g.dc-tooth")];
const ids = (c: HTMLElement) => toothEls(c).map((g) => g.getAttribute("data-tooth"));
const toothEl = (c: HTMLElement, id: string) => c.querySelector<SVGGElement>(`g.dc-tooth[data-tooth="${id}"]`)!;

describe("DentalChart", () => {
  it("draws a dog's 42 permanent teeth and a cat's 30, and adds the deciduous rows when one is charted", () => {
    const dog = render(<DentalChart dentition="canine" items={juno} />);
    expect(toothEls(dog.container)).toHaveLength(42);
    dog.unmount();

    const cat = render(<DentalChart dentition="feline" items={[f("307", "resorption", { grade: 4, detail: "crown_worse" })]} />);
    expect(toothEls(cat.container)).toHaveLength(30);
    expect(ids(cat.container)).not.toContain("105");
    expect(ids(cat.container)).not.toContain("305");
    cat.unmount();

    const pup = render(<DentalChart dentition="canine" items={[f("504", "persistent_deciduous")]} />);
    expect(toothEls(pup.container)).toHaveLength(70);
    expect(ids(pup.container)).toContain("808");
  });

  it("leaves uncharted teeth blank and marks findings with shapes", () => {
    const { container } = render(<DentalChart dentition="canine" items={juno} />);
    const blank = toothEl(container, "110");
    expect(blank.querySelectorAll(".dc-mark")).toHaveLength(0);
    expect(blank.hasAttribute("data-charted")).toBe(false);
    expect(toothEl(container, "209").querySelector('[data-glyph="circle"]')).not.toBeNull();
    const planned = toothEl(container, "304").querySelector('[data-glyph="x"]')!;
    expect(planned.getAttribute("stroke-dasharray")).toBe("3 2");
    expect(toothEl(container, "208").querySelector('[data-glyph="x"]')!.getAttribute("stroke-dasharray")).toBeNull();
    expect(toothEl(container, "208").querySelector('[data-glyph="zigzag"]')).not.toBeNull();
    // Normal is charted (ink outline) but carries no mark.
    expect(toothEl(container, "104").hasAttribute("data-charted")).toBe(true);
    expect(toothEl(container, "104").querySelectorAll(".dc-mark")).toHaveLength(0);
  });

  it("lights the evidence of all a tooth's findings on hover and focus", () => {
    const onEvidence = vi.fn();
    const { container } = render(<DentalChart dentition="canine" items={juno} onEvidence={onEvidence} />);
    fireEvent.mouseEnter(toothEl(container, "309"));
    expect(onEvidence).toHaveBeenLastCalledWith([7, 8]);
    fireEvent.mouseLeave(toothEl(container, "309"));
    expect(onEvidence).toHaveBeenLastCalledWith(null);
    fireEvent.focus(toothEl(container, "208"));
    expect(onEvidence).toHaveBeenLastCalledWith([4, 11]);
  });

  it("gives every tooth an accessible name", () => {
    const { container } = render(<DentalChart dentition="canine" items={juno} />);
    for (const g of toothEls(container)) expect(g.getAttribute("aria-label")).toMatch(/^\d{3}, /);
    expect(screen.getByRole("button", { name: "208, left maxillary fourth premolar: complicated crown fracture; open extraction" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "110, right maxillary second molar: not charted" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "304, left mandibular canine: planned: extraction" })).toBeTruthy();
  });

  it("lists findings by tooth in Triadan order with codes, and a legend of only the codes used", () => {
    const { container } = render(<DentalChart dentition="canine" items={juno} />);
    const lines = [...container.querySelectorAll("li.dc-line")].map((li) => li.textContent);
    expect(lines.map((l) => l?.slice(0, 3))).toEqual(["101", "102", "103", "104", "208", "209", "304", "309"]);
    expect(lines[4]).toBe("208Complicated crown fracture (T/FX/CCF); open extraction (XSS)");
    expect(lines[7]).toBe("309Periodontal disease stage 3 (PD3); mobility 2 (M2)");
    const legend = screen.getByRole("list", { name: "Legend" }).textContent ?? "";
    expect(legend).toContain("T/FX/CCF: complicated crown fracture");
    expect(legend).toContain("PD0-PD4: periodontal disease stage (AVDC)");
    expect(legend).toContain("Blank: not charted in this recording. It doesn't mean normal.");
    expect(legend).toContain("Right and left are the patient's.");
    expect(legend).not.toContain("F1-F3");
    expect(legend).not.toContain("TR1-TR5");
  });

  it("plays a finding's audio from the list", () => {
    const onPlay = vi.fn();
    render(<DentalChart dentition="canine" items={juno} onPlay={onPlay} />);
    fireEvent.click(screen.getByText("Open extraction (XSS)", { exact: false }));
    expect(onPlay).toHaveBeenCalledWith([11]);
  });

  it("shows a hard-flagged tooth with ! and the callout, and soft flags as a title", () => {
    const faithfulness: ChartFaithfulness = {
      ...clean,
      passed: false,
      unsupported_numbers: [{ path: "/dental_chart/0", number: "105", cited: [2], found_in: [] }],
      tooth_flags: [{ path: "/dental_chart/1", tooth: "108", kind: "name_conflict", other: "208" }],
    } as ChartFaithfulness;
    const items = [f("105", "missing"), f("108", "fracture", { detail: "complicated_crown" })];
    const { container } = render(<DentalChart dentition="canine" items={items} faithfulness={faithfulness} />);
    expect(toothEl(container, "105").querySelector('.dc-flag[data-kind="hard"]')?.textContent).toBe("!");
    expect(toothEl(container, "108").querySelector('.dc-flag[data-kind="soft"]')).not.toBeNull();
    expect(toothEl(container, "110").querySelector(".dc-flag")).toBeNull();
    expect(screen.getByText("105 isn't anywhere in the transcript. Check the recording and correct this before signing.")).toBeTruthy();
    const conflict = container.querySelector('li[data-tooth="108"] .stmt')!;
    expect(conflict.getAttribute("title")).toBe(
      "Said as 108 and as left maxillary fourth premolar (208). Check which tooth before signing.",
    );
    expect(conflict.hasAttribute("data-soft")).toBe(true);
  });

  it("draws no diagram without a dentition, and says so", () => {
    // Signed (not editable): no hint to edit what can't be edited.
    const signed = render(<DentalChart dentition={null} items={juno} />);
    expect(signed.container.querySelector("svg")).toBeNull();
    expect(screen.getByText("Dentition not recorded.")).toBeTruthy();
    expect(screen.queryByText(/Edit the chart/)).toBeNull();
    expect(signed.container.querySelectorAll("li.dc-line")).toHaveLength(8);
    expect(signed.container.textContent).not.toContain("Blank: not charted");
    signed.unmount();
    // A draft: how to choose, and the patient's species offered when it is a dog or a cat.
    const onChooseDentition = vi.fn();
    render(<DentalChart dentition={null} items={juno} editable suggestedDentition="feline" onChooseDentition={onChooseDentition} />);
    expect(screen.getByText("Dentition not recorded. Edit the chart to choose dog or cat.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "The patient is recorded as a cat: use a cat's chart" }));
    expect(onChooseDentition).toHaveBeenCalledWith("feline");
  });

  it("says Not discussed for an empty chart, with no diagram", () => {
    const { container } = render(<DentalChart dentition="canine" items={[]} />);
    expect(screen.getByText("Not discussed")).toBeTruthy();
    expect(container.querySelector("svg")).toBeNull();
  });

  it("splits the mouth into two blocks at phone width", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q === "(max-width: 767px)", addEventListener() {}, removeEventListener() {} }));
    const { container } = render(<DentalChart dentition="canine" items={juno} />);
    const blocks = [...container.querySelectorAll("svg.dc-svg")];
    expect(blocks.map((b) => b.getAttribute("data-block"))).toEqual(["right", "left"]);
    const inBlock = (b: Element) => [...b.querySelectorAll("g.dc-tooth")].map((g) => g.getAttribute("data-tooth")!);
    expect(inBlock(blocks[0]!).every((id) => id[0] === "1" || id[0] === "4")).toBe(true);
    expect(inBlock(blocks[1]!).every((id) => id[0] === "2" || id[0] === "3")).toBe(true);
    expect(inBlock(blocks[0]!)).toHaveLength(21);
    // Each block is 11 slots of 32 px: teeth stay at least 24 px wide on a 390 px phone.
    expect(blocks[0]!.getAttribute("viewBox")?.split(" ")[2]).toBe("352");
  });

  it("is one tab stop; arrows move between teeth; Enter and Space activate", () => {
    const onToothActivate = vi.fn();
    const { container } = render(<DentalChart dentition="canine" items={juno} onToothActivate={onToothActivate} />);
    const stops = toothEls(container).filter((g) => g.getAttribute("tabindex") === "0");
    expect(stops.map((g) => g.getAttribute("data-tooth"))).toEqual(["110"]);
    stops[0]!.focus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("109");
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("210");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("210");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("310");
    fireEvent.keyDown(document.activeElement!, { key: "Home" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("411");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(document.activeElement?.getAttribute("data-tooth")).toBe("110");
    expect(toothEls(container).filter((g) => g.getAttribute("tabindex") === "0")).toHaveLength(1);
    fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    expect(onToothActivate).toHaveBeenLastCalledWith("110", { additive: false });
    fireEvent.keyDown(document.activeElement!, { key: " " });
    expect(onToothActivate).toHaveBeenLastCalledWith("110", { additive: true });
    fireEvent.click(toothEl(container, "309"), { ctrlKey: true });
    expect(onToothActivate).toHaveBeenLastCalledWith("309", { additive: true });
    fireEvent.click(toothEl(container, "309"));
    expect(onToothActivate).toHaveBeenLastCalledWith("309", { additive: false });
  });

  it("highlights the selected teeth in the diagram and the list", () => {
    const { container } = render(<DentalChart dentition="canine" items={juno} selected={["309"]} onToothActivate={() => {}} toggles />);
    expect(toothEl(container, "309").hasAttribute("data-selected")).toBe(true);
    expect(toothEl(container, "309").getAttribute("aria-pressed")).toBe("true");
    expect(toothEl(container, "310").getAttribute("aria-pressed")).toBe("false");
    expect(container.querySelector('li[data-tooth="309"]')?.hasAttribute("data-selected")).toBe(true);
    expect(toothEls(container).find((g) => g.getAttribute("tabindex") === "0")?.getAttribute("data-tooth")).toBe("309");
  });

  it("announces teeth as toggles only where activating one toggles it", () => {
    // In a draft, a tooth opens the editor: a plain button, not a toggle.
    const { container } = render(<DentalChart dentition="canine" items={juno} selected={["309"]} onToothActivate={() => {}} />);
    expect(toothEl(container, "309").getAttribute("aria-pressed")).toBeNull();
  });

  it("splits the mouth by its own width, not the window's", () => {
    // A wide window, a narrow review pane.
    vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private cb: (entries: { contentRect: { width: number } }[]) => void) {}
        observe() {
          this.cb([{ contentRect: { width: 447 } }]);
        }
        disconnect() {}
      },
    );
    const { container } = render(<DentalChart dentition="canine" items={juno} />);
    expect([...container.querySelectorAll("svg.dc-svg")].map((b) => b.getAttribute("data-block"))).toEqual(["right", "left"]);
  });

  it("shows a tooth that blocks signing with a filled badge and says so to screen readers", () => {
    const faithfulness = {
      ...clean,
      passed: false,
      tooth_flags: [
        { path: "/dental_chart/0", tooth: "105", kind: "choose_dentition" },
        { path: "/dental_chart/1", tooth: "309", kind: "negated", expected: ["mobile"] },
        { path: "/dental_chart/2", tooth: "208", kind: "conflict", other: "/dental_chart/3", expected: ["Normal"] },
        { path: "/dental_chart/4", tooth: "307", kind: "other_type", expected: ["Open extraction"] },
        { path: "/dental_chart/5", tooth: "407", kind: "planned" },
        { path: "/dental_chart/6", tooth: "409", kind: "other_tooth", expected: ["grade 3"] },
      ],
    } as ChartFaithfulness;
    const items = [
      f("105", "missing"),
      f("309", "mobility"),
      f("208", "fracture"),
      f("208", "normal"),
      f("307", "extraction", { detail: "closed" }),
      f("407", "extraction"),
      f("409", "mobility", { grade: 3 }),
    ];
    const { container } = render(<DentalChart dentition="canine" items={items} faithfulness={faithfulness} />);
    expect(toothEl(container, "105").querySelector(".dc-flag-badge")).not.toBeNull();
    expect(toothEl(container, "309").querySelector(".dc-flag-badge")).toBeNull();
    expect(toothEl(container, "105").getAttribute("aria-label")).toMatch(/Blocks signing until it's corrected\.$/);
    expect(toothEl(container, "309").getAttribute("aria-label")).toMatch(/Needs checking before signing\.$/);
    expect(screen.getByText("Only a dog has 105. Edit the chart to choose dog or cat before signing.")).toBeTruthy();
    const title = (tooth: string) => container.querySelector(`li[data-tooth="${tooth}"] .stmt`)?.getAttribute("title");
    expect(title("309")).toBe("The cited line says this wasn't there. Check before signing.");
    expect(title("208")).toBe("This can't be right beside Normal on the same tooth. Check before signing.");
    expect(title("307")).toBe("The cited line also has the words for open extraction. Check the type before signing.");
    expect(title("407")).toBe("Charted as done, but the cited line sounds like a plan. Check before signing.");
    expect(title("409")).toBe("The cited line says this about another tooth. Check which tooth before signing.");
  });

  it("lets the note view render each finding as its own statement", () => {
    render(
      <DentalChart
        dentition="canine"
        items={[f("404", "normal", { evidence: [9] })]}
        renderStatement={(item, path, content) => <em data-path={path}>{content}</em>}
      />,
    );
    expect(screen.getByText("Normal").closest("em")?.getAttribute("data-path")).toBe("/dental_chart/0");
  });
});
