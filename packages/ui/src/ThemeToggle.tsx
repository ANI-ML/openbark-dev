import { useEffect, useState } from "react";
import { applyTheme, readTheme, type ThemeChoice } from "./theme";

const OPTIONS: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/** Explicit light/dark/system choice (DESIGN.md §9: user-overridable). */
export function ThemeToggle() {
  const [choice, setChoice] = useState<ThemeChoice>(() => readTheme());

  useEffect(() => {
    applyTheme(choice);
  }, [choice]);

  return (
    <div role="radiogroup" aria-label="Theme" className="ob-segmented">
      {OPTIONS.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={choice === o.value}
          className="ob-segment"
          onClick={() => setChoice(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
