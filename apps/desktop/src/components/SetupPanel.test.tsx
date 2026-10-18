import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const downloaded: string[] = [];
vi.mock("../engine", () => ({
  models: async () => [
    { id: "llm", name: "Gemma", slot: "llm", size_bytes: 5_000_000_000, fit: "fits", installed: false },
    { id: "vad", name: "Silero VAD", slot: "vad", size_bytes: 600_000, fit: "fits", installed: false },
    { id: "stt", name: "Parakeet", slot: "stt", size_bytes: 600_000_000, fit: "fits", installed: false },
  ],
  on: async () => () => {},
  downloadModel: async (id: string) => {
    downloaded.push(id);
  },
}));

import { SetupPanel } from "./SetupPanel";

afterEach(cleanup);

describe("SetupPanel", () => {
  it("downloads every missing model, smallest first, then reports ready", async () => {
    const onReady = vi.fn();
    render(
      <SetupPanel
        missing={[
          { id: "llm", name: "Gemma" },
          { id: "vad", name: "Silero VAD" },
          { id: "stt", name: "Parakeet" },
        ]}
        onReady={onReady}
      />,
    );
    const button = await screen.findByRole("button", { name: /Download everything \(5\.6 GB\)/ });
    fireEvent.click(button);
    await waitFor(() => expect(onReady).toHaveBeenCalled());
    expect(downloaded).toEqual(["vad", "stt", "llm"]);
    expect(screen.getAllByText("Ready")).toHaveLength(3);
  });
});
