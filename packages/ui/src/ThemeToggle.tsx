import { useEffect, useState } from "react";
import { applyTheme, readTheme, type ThemeChoice } from "./theme";

const OPTIONS: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/**
 * Explicit system/light/dark choice. Dark mode is a first-class target
 * (DESIGN.md §3.4) and honours prefers-color-scheme until the user picks one
 * (§10). A segmented control with text labels: sentence case, no icons.
 */
export function ThemeToggle({ translate = (s: string) => s }: { translate?: (english: string) => string } = {}) {
  const [choice, setChoice] = useState<ThemeChoice>(() => readTheme());

  useEffect(() => {
    applyTheme(choice);
  }, [choice]);

  return (
    <div role="radiogroup" aria-label={translate("Theme")} className="ob-segmented">
      {OPTIONS.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={choice === o.value}
          className="ob-segment"
          onClick={() => setChoice(o.value)}
        >
          {translate(o.label)}
        </button>
      ))}
    </div>
  );
}
