import { describe, expect, it } from "vitest";
import { translate } from "../i18n";
import { micNotice, micQuality, switchNote, type MicHealth } from "./health";
import { MicWatchdog } from "./watchdog";
import { micError, MIC_ERRORS } from "./errors";

const t = (s: string, v?: Record<string, string | number>) => translate("en", s, v);
const health = (over: Partial<MicHealth>): MicHealth => ({
  state: "ok",
  device: "MacBook Pro Microphone",
  sample_rate: 48000,
  quality: "good",
  switched_from: null,
  seconds: 1,
  ...over,
});

describe("micQuality (same rules as the engine)", () => {
  it("calls 8 kHz narrowband and a headset at 16 or 24 kHz hands-free", () => {
    expect(micQuality("Headset (AirPods Hands-Free AG Audio)", 8000)).toBe("narrowband");
    expect(micQuality("Headset (AirPods Hands-Free AG Audio)", 16000)).toBe("hands_free");
    expect(micQuality("AirPods Pro", 24000)).toBe("hands_free");
    expect(micQuality("Jabra Speak 510 USB", 16000)).toBe("good");
    expect(micQuality("Headset Microphone (USB)", 48000)).toBe("good");
    expect(micQuality("", 0)).toBe("good");
  });
});

describe("micNotice", () => {
  it("says nothing when all is well", () => {
    expect(micNotice(t, health({}), false)).toBeNull();
    expect(micNotice(t, null, false)).toBeNull();
  });

  it("names the microphone and the next step for each problem", () => {
    const lost = micNotice(t, health({ state: "lost" }), false)!;
    expect(lost.tone).toBe("danger");
    expect(lost.text).toMatch(/paused/);
    const silent = micNotice(t, health({ state: "silent" }), false)!;
    expect(silent.text).toMatch(/^MacBook Pro Microphone is sending silence/);
    expect(silent.text).toMatch(/Privacy & Security/);
    expect(micNotice(t, health({ state: "silent" }), true)!.text).toMatch(/this browser is allowed/);
    expect(micNotice(t, health({ state: "stalled" }), true)!.text).toMatch(/Keep this tab open/);
    expect(micNotice(t, health({ state: "stalled" }), false)!.text).toMatch(/reopening/);
  });

  it("warns, without alarming, about Bluetooth call quality", () => {
    const n = micNotice(t, health({ device: "AirPods", sample_rate: 8000, quality: "narrowband" }), false)!;
    expect(n.tone).toBe("warn");
    expect(n.text).toMatch(/telephone-quality audio \(8 kHz\)/);
    expect(micNotice(t, health({ device: "AirPods", sample_rate: 16000, quality: "hands_free" }), false)!.text).toMatch(
      /hands-free mode \(16 kHz\)/,
    );
  });

  it("is translated", () => {
    const fr = (s: string, v?: Record<string, string | number>) => translate("fr", s, v);
    expect(micNotice(fr, health({ state: "lost" }), false)!.text).toMatch(/pause/);
    expect(micNotice(fr, health({ state: "lost" }), false)!.text).not.toMatch(/recording is paused/);
  });
});

describe("switchNote", () => {
  it("announces a switch once", () => {
    const before = health({});
    const after = health({ device: "AirPods Pro", switched_from: "MacBook Pro Microphone" });
    expect(switchNote(t, before, after)).toBe("Now recording from AirPods Pro.");
    expect(switchNote(t, after, { ...after, seconds: 5 })).toBeNull();
    expect(switchNote(t, before, before)).toBeNull();
  });
});

describe("MicWatchdog", () => {
  it("warns about digital silence after 4 s and clears on sound", () => {
    const w = new MicWatchdog("USB mic", 48000, 0);
    expect(w.tick(0, 0)?.state).toBe("ok");
    for (let ms = 0; ms < 4000; ms += 50) {
      w.block(0, ms);
      expect(w.tick(ms, 0)).toBeNull();
    }
    w.block(0, 4050);
    expect(w.tick(4050, 4)?.state).toBe("silent");
    w.block(0.2, 4100);
    expect(w.tick(4100, 4)?.state).toBe("ok");
  });

  it("doesn't mistake a quiet room for silence", () => {
    const w = new MicWatchdog("USB mic", 48000, 0);
    w.tick(0, 0);
    for (let ms = 0; ms < 10000; ms += 50) {
      w.block(0.0005, ms);
      expect(w.tick(ms, 0)).toBeNull();
    }
  });

  it("reports no audio for 3 s as stalled, and a lost then replaced microphone", () => {
    const w = new MicWatchdog("Built-in", 48000, 0);
    w.block(0.1, 0);
    w.tick(0, 0);
    expect(w.tick(2900, 0)).toBeNull();
    expect(w.tick(3000, 0)?.state).toBe("stalled");
    w.lost();
    expect(w.tick(3100, 0)?.state).toBe("lost");
    w.opened("AirPods Pro", 16000, "Built-in", 4000);
    const h = w.tick(4000, 0)!;
    expect(h).toMatchObject({ state: "ok", device: "AirPods Pro", quality: "hands_free", switched_from: "Built-in" });
  });

  it("forgets silence from before a stall", () => {
    const w = new MicWatchdog("USB mic", 44100, 0);
    w.tick(0, 0);
    for (let ms = 0; ms < 3000; ms += 50) w.block(0, ms);
    expect(w.tick(6500, 0)?.state).toBe("stalled");
    w.block(0, 6600);
    expect(w.tick(6600, 0)?.state).toBe("ok");
  });
});

describe("micError", () => {
  const err = (name: string, message = "") => Object.assign(new Error(message), { name });

  it("tells denied, dismissed, missing and busy apart", () => {
    expect(micError(err("NotAllowedError", "Permission denied")).message).toBe(MIC_ERRORS.denied);
    expect(micError(err("NotAllowedError", "Permission dismissed")).message).toBe(MIC_ERRORS.dismissed);
    expect(micError(err("NotFoundError")).message).toBe(MIC_ERRORS.notFound);
    expect(micError(err("NotReadableError", "Could not start audio source")).message).toBe(MIC_ERRORS.busy);
    expect(micError(err("AbortError")).message).toBe(MIC_ERRORS.busy);
    expect(micError(err("SecurityError")).message).toBe(MIC_ERRORS.denied);
    expect(micError(err("NotSupportedError")).message).toBe(MIC_ERRORS.unsupported);
  });

  it("every message is translated in every interface language", () => {
    for (const lang of ["de", "es", "fr", "pt"]) {
      for (const text of Object.values(MIC_ERRORS)) {
        expect(translate(lang, text), `${lang}: ${text}`).not.toBe(text);
      }
    }
  });
});
