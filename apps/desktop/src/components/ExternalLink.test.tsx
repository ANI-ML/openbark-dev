import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as engine from "../engine";
import { ExternalLink, NVIDIA_LICENCE } from "./ExternalLink";

// The desktop app: a plain target="_blank" link does nothing there, because
// the main window has no opener permission (capabilities/default.json).
vi.mock("../engine", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../engine")>()),
  isDesktop: true,
  openWebPage: vi.fn().mockResolvedValue(undefined),
}));

const mocked = vi.mocked(engine);
afterEach(cleanup);

describe("ExternalLink on the desktop", () => {
  it("asks the app to open the page it names, and keeps the address for the browser", () => {
    render(
      <ExternalLink href={NVIDIA_LICENCE} page={{ page: "nvidia-licence" }}>
        (read it)
      </ExternalLink>,
    );
    const link = screen.getByRole("link", { name: "(read it)" });
    expect(link.getAttribute("href")).toBe(NVIDIA_LICENCE);
    expect(link.getAttribute("target")).toBe("_blank");
    // false: the default was prevented, so the opener plugin's own handler
    // (which the window isn't allowed to use) never runs.
    expect(fireEvent.click(link)).toBe(false);
    expect(mocked.openWebPage).toHaveBeenCalledWith({ page: "nvidia-licence" });
  });

  it("passes a Hugging Face model's id, never an address", () => {
    render(
      <ExternalLink href="https://huggingface.co/google/gemma" page={{ page: "hf-repo", repo: "google/gemma" }}>
        Request access
      </ExternalLink>,
    );
    fireEvent.click(screen.getByRole("link", { name: "Request access" }));
    expect(mocked.openWebPage).toHaveBeenLastCalledWith({ page: "hf-repo", repo: "google/gemma" });
  });
});
