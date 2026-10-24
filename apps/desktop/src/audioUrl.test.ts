import { afterEach, describe, expect, it, vi } from "vitest";
import type { SessionView } from "./engine";

const visit = { id: "2026-10-20_101500_playback-check" } as SessionView;

/** engine.ts decides desktop or web once, when it loads. */
async function loadEngine(desktop: boolean) {
  vi.resetModules();
  const calls: { cmd: string; args: unknown }[] = [];
  if (desktop) {
    vi.stubGlobal("__TAURI_INTERNALS__", {
      // Tauri's own: obaudio://localhost/<id>, or http://obaudio.localhost/<id> on Windows.
      convertFileSrc: (path: string, protocol: string) => `${protocol}://localhost/${encodeURIComponent(path)}`,
      invoke: async (cmd: string, args: unknown) => {
        calls.push({ cmd, args });
        return new ArrayBuffer(8);
      },
      transformCallback: () => 0,
    });
  }
  return { engine: await import("./engine"), calls };
}

describe("the recording's URL for the player", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("desktop: the app's ranged audio scheme, without copying the recording into memory", async () => {
    const { engine, calls } = await loadEngine(true);
    expect(engine.isDesktop).toBe(true);
    expect(await engine.audioUrl(visit)).toBe(`obaudio://localhost/${visit.id}`);
    expect(calls).toEqual([]);
  });

  it("desktop fallback: the whole recording from memory, as a blob: URL", async () => {
    const { engine, calls } = await loadEngine(true);
    const created = vi.fn(() => "blob:tauri://localhost/1");
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: created }));
    expect(await engine.audioFallbackUrl(visit)).toBe("blob:tauri://localhost/1");
    expect(calls).toEqual([{ cmd: "session_audio", args: { id: visit.id } }]);
  });

  it("Linux desktop: WebKitGTK can't play the app's scheme, so it starts from the blob: URL", async () => {
    const { engine, calls } = await loadEngine(true);
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15" });
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: () => "blob:tauri://localhost/2" }));
    expect(await engine.audioUrl(visit)).toBe("blob:tauri://localhost/2");
    expect(calls).toEqual([{ cmd: "session_audio", args: { id: visit.id } }]);
  });

  it("web mode: the server's ranged route, and no fallback", async () => {
    const { engine } = await loadEngine(false);
    expect(engine.isDesktop).toBe(false);
    expect(await engine.audioUrl(visit)).toBe(`/api/sessions/${visit.id}/audio`);
    expect(await engine.audioFallbackUrl(visit)).toBeNull();
  });
});
