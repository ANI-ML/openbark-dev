import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DeleteVisitMenu } from "./DeleteVisitMenu";

afterEach(cleanup);

const REASONS = [
  "Test or practice recording",
  "Recorded by mistake",
  "Duplicate visit",
  "Client withdrew consent",
  "Another reason",
];

describe("DeleteVisitMenu", () => {
  it("lists the five reasons, then the hint, and deletes with the one chosen", () => {
    const onDelete = vi.fn();
    render(<DeleteVisitMenu onDelete={onDelete} />);
    const trigger = screen.getByRole("button", { name: "Delete visit" });
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    fireEvent.click(trigger);
    expect(screen.getByText("Why delete this visit?")).toBeTruthy();
    const items = screen.getAllByRole("menuitem").map((b) => b.textContent);
    expect(items).toEqual(REASONS);
    const menu = screen.getByRole("menu").parentElement!;
    const text = menu.textContent ?? "";
    expect(text.indexOf("Another reason")).toBeLessThan(
      text.indexOf("Filed under the wrong patient? Use Edit details instead."),
    );
    expect(text).toContain("It moves to Deleted visits for 30 days.");
    expect(text).not.toContain("also has notes for");
    fireEvent.click(screen.getByRole("menuitem", { name: "Recorded by mistake" }));
    expect(onDelete).toHaveBeenCalledWith("mistake");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("names the other animals in a multi-pet visit", () => {
    render(<DeleteVisitMenu others="Olive" onDelete={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete visit" }));
    expect(screen.getByText("This visit also has notes for Olive. They're deleted with it.")).toBeTruthy();
  });

  it("moves with the arrow keys, and Escape returns focus to Delete visit", () => {
    render(<DeleteVisitMenu onDelete={() => {}} />);
    const trigger = screen.getByRole("button", { name: "Delete visit" });
    fireEvent.click(trigger);
    const items = screen.getAllByRole("menuitem");
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(items[0]!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1]!, { key: "ArrowUp" });
    fireEvent.keyDown(items[0]!, { key: "ArrowUp" });
    expect(document.activeElement).toBe(items[4]);
    fireEvent.keyDown(items[4]!, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  });

  it("is disabled with the reason as its tooltip on a signed visit", () => {
    const why = "Signed visits can't be deleted yet. Add an addendum to correct the note.";
    const onDelete = vi.fn();
    render(<DeleteVisitMenu disabledReason={why} onDelete={onDelete} />);
    const trigger = screen.getByRole("button", { name: "Delete visit" }) as HTMLButtonElement;
    expect(trigger.disabled).toBe(true);
    expect(trigger.closest("[title]")?.getAttribute("title")).toBe(why);
    expect(document.getElementById(trigger.getAttribute("aria-describedby") ?? "")?.textContent).toBe(why);
    fireEvent.click(trigger);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
