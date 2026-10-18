import { Button } from "@openbark/ui";
import { useApp } from "../context";
import { useT } from "../i18n";
import { describeModel, LEGEND, recommendedSize, STARTERS } from "../modelGuide";

/** A model name as plain-language chips; each chip's meaning shows on hover and to screen readers. */
export function ModelTags({ name, only }: { name: string; only?: "compression" }) {
  const t = useT();
  const tags = describeModel(name).filter((tag) => !only || /^(I?Q\d|\d-bit|BF16|F16|FP16|F32)/.test(tag.label));
  if (tags.length === 0) return null;
  return (
    <span className="model-tags">
      {tags.map((tag) => {
        const meaning = t(tag.meaning, tag.vars);
        return (
          <span key={tag.label} className="ob-chip model-tag" title={meaning} aria-label={`${tag.label}: ${meaning}`}>
            {tag.label}
          </span>
        );
      })}
    </span>
  );
}

/** "What this name means", written out (one model, where there's room). */
export function ModelMeaning({ name }: { name: string }) {
  const t = useT();
  const tags = describeModel(name);
  if (tags.length === 0) return null;
  return (
    <dl className="model-meaning">
      {tags.map((tag) => (
        <div key={tag.label}>
          <dt className="mono">{tag.label}</dt>
          <dd className="small">{t(tag.meaning, tag.vars)}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * The model legend (Models screen): what 4B, 12B, Q4 and the rest mean, the
 * size that suits this computer, and good starting points on Hugging Face.
 */
export function ModelGuide({ onOpenRepo }: { onOpenRepo: (repo: string) => void }) {
  const app = useApp();
  const t = useT();
  const hw = app.status?.hardware;
  const mac = app.status?.platform === "macos";
  const size = hw ? recommendedSize(hw.gpu_memory_bytes, hw.ram_bytes, mac) : null;
  return (
    <details className="group model-guide">
      <summary>
        <h2>{t("What the model names mean")}</h2>
        <span className="small muted">{t("4B or 12B, Q4, QAT, GGUF: a short guide, and which model suits this computer")}</span>
      </summary>
      {size && hw && (
        <p className="ob-notice" data-tone="ok">
          {t("For this computer ({hardware}), start with Gemma 4 {size}.", { hardware: hw.summary, size })}{" "}
          {size === "12B"
            ? t("It has room for the larger, more careful model; E4B is faster if you prefer speed.")
            : size === "E4B"
              ? t("It's OpenBark's everyday choice; 12B needs more graphics memory than this computer has.")
              : t("It's the lightest choice; larger models would be slow here.")}
        </p>
      )}
      <dl className="legend">
        {LEGEND.map((row) => (
          <div key={row.term}>
            <dt className="mono">{t(row.term)}</dt>
            <dd className="small">{t(row.meaning)}</dd>
          </div>
        ))}
      </dl>
      <h3>{t("Good starting points on Hugging Face")}</h3>
      <ul className="starters">
        {STARTERS.map((s) => (
          <li key={s.repo} data-recommended={s.size === size || undefined}>
            <div className="starter-text">
              <span className="mono starter-repo">{s.repo}</span>
              <span className="small muted">{t(s.why)}</span>
            </div>
            <Button variant="ghost" onClick={() => onOpenRepo(s.repo)}>
              {t("Open")}
            </Button>
          </li>
        ))}
      </ul>
    </details>
  );
}
