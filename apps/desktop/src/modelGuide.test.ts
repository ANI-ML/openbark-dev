import { describe, expect, it } from "vitest";
import { describeModel, recommendedSize } from "./modelGuide";

const labels = (name: string) => describeModel(name).map((t) => t.label);

describe("model names in plain language", () => {
  it("reads size, tuning and compression", () => {
    expect(labels("gemma-4-12b-it-Q4_K_M.gguf")).toEqual(["12B", "it", "Q4_K_M"]);
    expect(labels("google/gemma-4-E4B-it-qat-q4_0-gguf")).toEqual(["E4B", "it", "QAT", "Q4_0", "GGUF"]);
    expect(labels("unsloth/gemma-4-26B-A4B-it-GGUF")).toEqual(["26B", "A4B", "it", "GGUF"]);
    expect(labels("lmstudio-community/gemma-4-E4B-it-MLX-4bit")).toEqual(["E4B", "it", "4-bit", "MLX"]);
    expect(labels("Llama-3.2-3B-Instruct-Q8_0")).toEqual(["3B", "it", "Q8_0"]);
    expect(labels("Qwen3-0.6B-base-IQ3_XS")).toEqual(["0.6B", "base", "IQ3_XS"]);
  });

  it("explains each part", () => {
    const [size] = describeModel("gemma-4-12b-it");
    expect(size?.vars?.n).toBe("12");
    expect(size?.meaning).toMatch(/billion parameters/);
  });

  it("suggests a size for the computer", () => {
    expect(recommendedSize(16e9, 32e9, false)).toBe("12B");
    expect(recommendedSize(6e9, 16e9, false)).toBe("E4B");
    expect(recommendedSize(null, 8e9, false)).toBe("E2B");
    expect(recommendedSize(null, 36e9, true)).toBe("12B");
    expect(recommendedSize(null, 16e9, true)).toBe("E4B");
  });
});
