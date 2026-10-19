import { afterEach, describe, expect, it, vi } from "vitest";
import * as engine from "./engine";

/** Web mode: every template call says who is asking (ADR-0024: each person resolves their own version). */
describe("template calls send the person using the app", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    engine.setActor(null);
  });

  function stubFetch() {
    const calls: { url: string; body: unknown }[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init?: RequestInit) => {
        calls.push({ url, body: init?.body ? JSON.parse(String(init.body)) : null });
        return new Response("{}", { headers: { "content-type": "application/json" } });
      }),
    );
    return calls;
  }

  it("reads with ?by= and writes with by in the body; a PIN only ever in a body", async () => {
    const calls = stubFetch();
    engine.setActor("u-sam");
    await engine.listTemplates();
    await engine.template("soap");
    await engine.saveTemplate({ template: { id: "soap", version: "1", name: "SOAP", sections: [] }, base_version: "1" });
    await engine.shareTemplate("soap", "replace", null, { id: "u-vet", pin: "2468" });
    await engine.resetTemplate("soap");
    await engine.compareTemplate("soap", "base");
    expect(calls[0]!.url).toBe("/api/templates?by=u-sam");
    expect(calls[1]!.url).toBe("/api/templates/soap?by=u-sam");
    expect(calls[2]!.body).toMatchObject({ by: "u-sam", base_version: "1" });
    expect(calls[3]!.url).toBe("/api/templates/soap/share");
    expect(calls[3]!.body).toMatchObject({ by: "u-sam", approver: { id: "u-vet", pin: "2468" } });
    expect(calls[4]!.body).toEqual({ by: "u-sam" });
    expect(calls[5]!.url).toBe("/api/templates/soap/compare?by=u-sam&with=base");
    for (const c of calls) expect(c.url).not.toContain("2468");
  });
});
