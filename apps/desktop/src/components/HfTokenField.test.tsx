import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as engine from "../engine";
import { HfTokenField } from "./HfTokenField";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return { ...real, hfTokenStatus: vi.fn(), setHfToken: vi.fn(), clearHfToken: vi.fn() };
});
const mocked = vi.mocked(engine);

const NONE: engine.HfTokenStatus = { saved: false, source: null, user: null };
const TOKEN = "hf_abcdefghijklmnopqrstuvwxyz0123";

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("HfTokenField", () => {
  it("saves a token, never shows it back, and says whose it is", async () => {
    mocked.hfTokenStatus.mockResolvedValue(NONE);
    mocked.setHfToken.mockResolvedValue({ saved: true, source: "keychain", user: "maple-clinic" });
    const onChange = vi.fn();
    render(<HfTokenField onChange={onChange} />);
    const input = (await screen.findByLabelText("Hugging Face token")) as HTMLInputElement;
    expect(input.type).toBe("password");
    expect(screen.getByText("A Read token is enough.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Remove token" })).toBeNull();
    fireEvent.change(input, { target: { value: ` ${TOKEN} ` } });
    fireEvent.click(screen.getByRole("button", { name: "Save token" }));
    await waitFor(() => expect(mocked.setHfToken).toHaveBeenCalledWith(TOKEN));
    expect(await screen.findByText("Saved in this computer's keychain. Signed in as maple-clinic.")).toBeTruthy();
    expect(input.value).toBe("");
    expect(document.body.textContent).not.toContain(TOKEN);
    expect(onChange).toHaveBeenCalledWith({ saved: true, source: "keychain", user: "maple-clinic" });
  });

  it.each([
    ["keychain", "Saved in this computer's keychain."],
    ["vault_file", "Saved, encrypted, on this computer."],
    ["session", "Kept until OpenBark closes."],
  ] as const)("says where a saved token is kept (%s), and can remove it", async (source, line) => {
    mocked.hfTokenStatus.mockResolvedValue({ saved: true, source, user: null });
    mocked.clearHfToken.mockResolvedValue(NONE);
    render(<HfTokenField />);
    expect(await screen.findByText(line)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove token" }));
    await waitFor(() => expect(mocked.clearHfToken).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByText(line)).toBeNull());
  });

  it("a token set by the server can't be changed here", async () => {
    mocked.hfTokenStatus.mockResolvedValue({ saved: true, source: "server", user: null });
    render(<HfTokenField />);
    expect(await screen.findByText("Set by the server.")).toBeTruthy();
    expect(screen.queryByLabelText("Hugging Face token")).toBeNull();
    expect(screen.queryByRole("button", { name: "Save token" })).toBeNull();
  });

  it("shows when Hugging Face doesn't accept the token", async () => {
    mocked.hfTokenStatus.mockResolvedValue(NONE);
    mocked.setHfToken.mockRejectedValue(
      new Error("Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens."),
    );
    render(<HfTokenField />);
    fireEvent.change(await screen.findByLabelText("Hugging Face token"), { target: { value: "hf_wrong_token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save token" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens.",
    );
  });
});
