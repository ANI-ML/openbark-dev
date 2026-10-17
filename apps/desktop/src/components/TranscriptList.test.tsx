import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { HintedText } from "./TranscriptList";

afterEach(cleanup);

it("underlines hinted words and explains the suggestion without changing the text", () => {
  const { container } = render(
    <HintedText
      text="an anti-nausea injection, marapitin, at one milligram per kilo"
      hints={[{ heard: "marapitin", likely: ["maropitant"], kind: "drug" }]}
    />,
  );
  expect(container.textContent).toBe("an anti-nausea injection, marapitin, at one milligram per kilo");
  const hint = screen.getByLabelText('Heard "marapitin". Likely maropitant.');
  expect(hint.textContent).toBe("marapitin");
});
