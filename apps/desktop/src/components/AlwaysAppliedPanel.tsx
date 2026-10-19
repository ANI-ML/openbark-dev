import { Lock } from "lucide-react";
import type { AlwaysApplied } from "../engine";
import { useT } from "../i18n";

/**
 * The rules that always apply (ADR-0024 section 4.6): the core rules in
 * plain words, this template's fixed rules, and the checks after drafting.
 * Read-only: no inputs, a lock beside each. "Exact wording sent to the
 * model" opens the English the model gets.
 */
export function AlwaysAppliedPanel({ rules }: { rules: AlwaysApplied }) {
  const t = useT();
  return (
    <section className="always-applied" aria-labelledby="always-title">
      <h3 id="always-title" className="label">
        <Lock size={16} aria-hidden /> {t("Always applied")}
      </h3>
      <p className="small muted">{t("These keep every note to what was said. They apply to every template and can't be changed.")}</p>
      <ul className="always-list">
        {rules.core.map((r) => (
          <li key={r.text}>
            <Lock size={14} aria-hidden className="faint" />
            <span className="small">{t(r.summary)}</span>
          </li>
        ))}
        {rules.fixed.map((r) => (
          <li key={r}>
            <Lock size={14} aria-hidden className="faint" />
            <span className="small">{r}</span>
          </li>
        ))}
      </ul>
      <p className="small muted">{rules.checks.map((c) => t(c)).join(" ")}</p>
      <details className="always-exact">
        <summary className="small">{t("Exact wording sent to the model")}</summary>
        <pre className="prompt">{[...rules.core.map((r) => `- ${r.text}`), ...rules.fixed.map((r) => `- ${r}`)].join("\n")}</pre>
      </details>
    </section>
  );
}
