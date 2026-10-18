import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DeleteRecordingDialog } from "./DeleteRecordingDialog";

afterEach(cleanup);

const IMPORTED = "The file you imported, maple.m4a, is still where it was. Delete it there too if you need to.";
const UNSIGNED = "The note isn't signed yet. Listen back to anything you want to check first.";

describe("DeleteRecordingDialog", () => {
  it("says the note stays, and adds the imported-file and unsigned lines only when they apply", () => {
    render(
      <DeleteRecordingDialog signed={false} multiPet={false} importedFile="maple.m4a" onConfirm={vi.fn()} onClose={() => {}} />,
    );
    expect(screen.getByRole("heading").textContent).toBe("Delete this recording? The note stays.");
    expect(
      screen.getByText(
        "The transcript, drafts and signed notes are kept. You won't be able to play this visit back or transcribe it again.",
      ),
    ).toBeTruthy();
    expect(screen.getByText(IMPORTED)).toBeTruthy();
    expect(screen.getByText(UNSIGNED)).toBeTruthy();
    expect(screen.queryByText("The recording covers every animal in this visit.")).toBeNull();
    cleanup();
    render(<DeleteRecordingDialog signed multiPet importedFile={null} onConfirm={vi.fn()} onClose={() => {}} />);
    expect(screen.queryByText(IMPORTED)).toBeNull();
    expect(screen.queryByText(UNSIGNED)).toBeNull();
    expect(screen.getByText("The recording covers every animal in this visit.")).toBeTruthy();
  });

  it("keeps the recording by default and deletes only on the destructive button", async () => {
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    render(<DeleteRecordingDialog signed multiPet={false} importedFile={null} onConfirm={onConfirm} onClose={() => {}} />);
    const keep = screen.getByRole("button", { name: "Keep recording" });
    expect(document.activeElement).toBe(keep);
    const del = screen.getByRole("button", { name: "Delete recording" });
    expect(del.getAttribute("data-variant")).toBe("danger");
    fireEvent.click(del);
    await waitFor(() => expect(onConfirm).toHaveBeenCalledTimes(1));
  });

  it("shows why it couldn't delete, and stays open", async () => {
    const onConfirm = vi.fn().mockRejectedValue(new Error("This visit has no recording."));
    render(<DeleteRecordingDialog signed multiPet={false} importedFile={null} onConfirm={onConfirm} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete recording" }));
    expect((await screen.findByRole("alert")).textContent).toBe("This visit has no recording.");
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});
