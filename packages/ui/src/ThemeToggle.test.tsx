import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { ThemeToggle } from "./ThemeToggle";

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("ThemeToggle", () => {
  it("defaults to following the system", () => {
    render(<ThemeToggle />);
    expect(screen.getByRole("radio", { name: "System" }).getAttribute("aria-checked")).toBe("true");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });

  it("applies and remembers an explicit choice", async () => {
    render(<ThemeToggle />);
    await userEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("openbark.theme")).toBe("dark");

    await userEvent.click(screen.getByRole("radio", { name: "System" }));
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
    expect(localStorage.getItem("openbark.theme")).toBeNull();
  });
});
