/**
 * Plain-language help with model names, for people who don't follow AI
 * models. A name like "gemma-4-12b-it-Q4_K_M" packs several facts; each
 * becomes a short label with its meaning. Strings are English and
 * translated where shown.
 */

export interface ModelTag {
  /** What appears on the chip ("12B", "Q4_K_M"). */
  label: string;
  /** One-line meaning, translated where shown, with {n}-style placeholders filled from vars. */
  meaning: string;
  vars?: Record<string, string>;
}

const num = (s: string) => s.replace(/^0+(?=\d)/, "");

/** The facts in a model's name, in reading order. */
export function describeModel(name: string): ModelTag[] {
  const tags: ModelTag[] = [];
  const n = name.replace(/\.gguf$/i, "");
  // Size: "12b", "E4B" (Gemma's effective size), "26B-A4B" (mixture of experts).
  const sized = n.match(/(?:^|[^a-z0-9.])(e)?(\d+(?:\.\d+)?)b(?![a-z0-9])/i);
  const active = n.match(/(?:^|[^a-z0-9])a(\d+(?:\.\d+)?)b(?![a-z0-9])/i);
  if (sized?.[2]) {
    const size = num(sized[2]);
    if (sized[1]) {
      tags.push({
        label: `E${size}B`,
        meaning: "About {n} billion parameters in effect: Gemma's efficient design runs like a {n}B model and knows a little more",
        vars: { n: size },
      });
    } else {
      tags.push({
        label: `${size}B`,
        meaning: "{n} billion parameters: the model's size. Bigger is more careful with complex visits, but slower and needs more memory",
        vars: { n: size },
      });
    }
  }
  if (active?.[1] && sized?.[2] && active.index !== sized.index) {
    tags.push({
      label: `A${num(active[1])}B`,
      meaning: "Only {n} billion parameters work on each word, so it runs faster than its full size suggests",
      vars: { n: num(active[1]) },
    });
  }
  if (/(?:^|[^a-z])(it|instruct|chat)(?:$|[^a-z])/i.test(n)) {
    tags.push({ label: "it", meaning: "Instruction-tuned: trained to follow instructions. This is the kind that writes notes" });
  } else if (/(?:^|[^a-z])(pt|base)(?:$|[^a-z])/i.test(n)) {
    tags.push({ label: "base", meaning: "A base model, not trained to follow instructions. Not suited to writing notes" });
  }
  if (/qat/i.test(n)) {
    tags.push({ label: "QAT", meaning: "Trained to be compressed, so the small file keeps nearly full quality" });
  }
  const quant = n.match(/(?:^|[^a-z0-9])(i?q\d(?:_[a-z0-9]+)*)(?![a-z0-9])/i);
  const bits = n.match(/(?:^|[^a-z0-9])(\d)-?bit(?![a-z0-9])/i);
  const full = n.match(/(?:^|[^a-z0-9])(bf16|f16|fp16|f32)(?![a-z0-9])/i);
  if (quant?.[1]) {
    const q = quant[1].toUpperCase();
    const b = q.match(/Q(\d)/)?.[1] ?? "4";
    tags.push({
      label: q,
      meaning:
        b === "8"
          ? "Compressed to 8 bits: near-original quality, about twice the size of 4-bit"
          : b === "4"
            ? "Compressed to 4 bits: about a quarter of the original size, with a small loss. The usual choice"
            : Number(b) < 4
              ? "Compressed below 4 bits: smaller and faster, but noticeably less careful"
              : "Compressed to {n} bits: a little larger and more careful than 4-bit",
      vars: { n: b },
    });
  } else if (bits?.[1]) {
    tags.push({
      label: `${bits[1]}-bit`,
      meaning: "Compressed to {n} bits per number (smaller numbers mean a smaller, slightly less careful model)",
      vars: { n: bits[1] },
    });
  } else if (full?.[1]) {
    tags.push({ label: full[1].toUpperCase(), meaning: "Uncompressed: full quality, but very large and slow on most computers" });
  }
  if (/gguf/i.test(n)) tags.push({ label: "GGUF", meaning: "The file format OpenBark runs, on any computer" });
  if (/mlx/i.test(n)) tags.push({ label: "MLX", meaning: "Apple's format for Mac (shown on a Mac only)" });
  return tags;
}

/** Which note model suits this computer (memory in bytes). */
export function recommendedSize(gpuBytes: number | null | undefined, ramBytes: number, mac: boolean): "12B" | "E4B" | "E2B" {
  const gb = (b: number | null | undefined) => (b ?? 0) / 1e9;
  // On a Mac the graphics chip shares the computer's memory.
  const usable = mac ? gb(ramBytes) * 0.6 : Math.max(gb(gpuBytes), 0);
  if (usable >= 10) return "12B";
  if (usable >= 5 || gb(ramBytes) >= 16) return "E4B";
  return "E2B";
}

/** The legend: each term, what it means, and an everyday comparison. */
export const LEGEND: { term: string; meaning: string }[] = [
  {
    term: "2B, 4B, 12B, 27B",
    meaning:
      "The model's size, in billions of parameters (the settings it learned). Like a clinician's experience: a bigger model handles long or complicated visits more carefully, but it's slower and needs more memory.",
  },
  {
    term: "E2B, E4B",
    meaning: "Gemma's efficient sizes: they run like a 2B or 4B model but know a little more. E4B is OpenBark's everyday choice.",
  },
  {
    term: "26B-A4B",
    meaning: "A large model where only part works on each word (4B of 26B), so it's faster than its size suggests but still needs memory for all of it.",
  },
  {
    term: "it, instruct",
    meaning: "Trained to follow instructions. Note writing needs these; skip models marked base or pt.",
  },
  {
    term: "Q4_0, Q4_K_M, Q8_0, 4-bit",
    meaning:
      "How much the file is compressed. 4-bit is about a quarter of the original size with a small loss and is the usual choice; 8-bit is close to the original at twice the size. K_M is a good default variant.",
  },
  {
    term: "QAT",
    meaning: "Trained to be compressed: a 4-bit file that keeps nearly full quality. Prefer it when offered.",
  },
  {
    term: "GGUF, MLX",
    meaning: "File formats. OpenBark runs GGUF on any computer. MLX is Apple's format, labelled on a Mac.",
  },
  {
    term: "Fits, Tight, Won't fit",
    meaning: "Whether the model fits this computer's graphics memory (or a Mac's shared memory). Tight works but leaves little room for other programs.",
  },
  {
    term: "Licence",
    meaning: "The terms you use it under. Apache-2.0 and MIT allow clinic use and download with one click; others ask you to read and accept their terms.",
  },
];

/** Good starting points on Hugging Face, by computer (searchable there). */
/** Starting points on a phone (ADR-0018 section 10): small GGUFs that fit its memory. */
export const PHONE_STARTERS: { repo: string; label: string; why: string }[] = [
  { repo: "google/gemma-4-E2B-it-qat-q4_0-gguf", label: "Gemma 4 E2B", why: "iPhones with 8 GB or more: about 3.4 GB" },
  { repo: "unsloth/Qwen3-1.7B-GGUF", label: "Qwen3 1.7B", why: "iPhones with 6 GB: about 1.1 GB" },
];

export const STARTERS: { repo: string; size: "E2B" | "E4B" | "12B"; why: string }[] = [
  { repo: "google/gemma-4-E2B-it-qat-q4_0-gguf", size: "E2B", why: "Older laptops and small computers: fastest, about 3 GB" },
  { repo: "google/gemma-4-E4B-it-qat-q4_0-gguf", size: "E4B", why: "Most laptops: OpenBark's default, about 5 GB" },
  { repo: "google/gemma-4-12b-it-qat-q4_0-gguf", size: "12B", why: "A graphics card with 10 GB or more, or a Mac with 24 GB or more: more careful, about 7 GB" },
];
