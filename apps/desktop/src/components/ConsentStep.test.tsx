import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConsentStep } from "./ConsentStep";

afterEach(cleanup);

const step = (rule: "all_parties" | "one_party", signer = "Dr Patel") => {
  const onConfirm = vi.fn();
  const onDecline = vi.fn();
  render(
    <ConsentStep
      rule={rule}
      script="May I record?"
      signer={signer}
      purpose="record"
      onConfirm={onConfirm}
      onDecline={onDecline}
      onCancel={() => {}}
    />,
  );
  return { onConfirm, onDecline };
};

describe("ConsentStep", () => {
  it("under an all-parties rule needs the client: no clinician-only option", () => {
    const { onConfirm, onDecline } = step("all_parties");
    expect(screen.getByText("May I record?")).toBeTruthy();
    expect(screen.queryByLabelText(/Only I consent/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Client agreed, start recording" }));
    expect(onConfirm).toHaveBeenCalledWith(
      { rule: "all_parties", method: "verbal", confirmed_by: "Dr Patel", script: "May I record?" },
      "Dr Patel",
    );
    fireEvent.click(screen.getByRole("button", { name: "Client declined" }));
    expect(onDecline).toHaveBeenCalled();
  });

  it("under a one-party rule allows clinician-only consent", () => {
    const { onConfirm } = step("one_party");
    fireEvent.click(screen.getByLabelText(/Only I consent/));
    expect(screen.queryByRole("button", { name: "Client declined" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    expect(onConfirm.mock.calls[0]?.[0]).toMatchObject({ rule: "one_party", method: "clinician_only", script: null });
  });

  it("asks for a name when Settings has none", () => {
    const { onConfirm } = step("all_parties", "");
    const go = screen.getByRole("button", { name: "Client agreed, start recording" }) as HTMLButtonElement;
    expect(go.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText(/Your name/), { target: { value: "Dr Lee" } });
    fireEvent.click(go);
    expect(onConfirm.mock.calls[0]?.[1]).toBe("Dr Lee");
  });
});
