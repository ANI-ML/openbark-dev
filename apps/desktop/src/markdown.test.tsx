import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { NoteMarkdown } from "./markdown";

afterEach(cleanup);

const SAMPLE = `# SOAP note

*Draft: not reviewed or signed by a veterinarian.*

> ⚠ 1 item(s) could not be matched to the transcript and are marked below.

## Objective

**Vitals**

- Temperature: 39.1 °C [12]
- HR: 240 bpm [12] ⚠ *check against recording*
- Not discussed: RR
`;

it("renders headings, lists, citations and the draft notice", () => {
  const { container } = render(<NoteMarkdown source={SAMPLE} />);
  expect(container.querySelector("h1")?.textContent).toBe("SOAP note");
  expect(container.querySelector("h2")?.textContent).toBe("Objective");
  expect(container.querySelectorAll("li")).toHaveLength(3);
  expect(container.querySelector(".cite")?.textContent).toBe("[12]");
  expect(container.querySelector(".ob-notice")?.textContent).toContain("could not be matched");
  expect(container.querySelector("em")?.textContent).toContain("Draft");
});

it("never injects HTML from note text", () => {
  const { container } = render(<NoteMarkdown source={"- <img src=x onerror=alert(1)>"} />);
  expect(container.querySelector("img")).toBeNull();
  expect(container.textContent).toContain("<img");
});
