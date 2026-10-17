import { useEffect, useState } from "react";
import { applyTheme, readTheme, type ThemeChoice } from "./theme";

const OPTIONS: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/** Explicit light/dark/system switch (a segmented control). */
export function ThemeToggle() {
  const [choice, setChoice] = useState<ThemeChoice>(() => readTheme());

  useEffect(() => {
    applyTheme(choice);
  }, [choice]);

  return (
    <div role="radiogroup" aria-label="Theme" style={{ display: "inline-flex", gap: 2 }}>
      {OPTIONS.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={choice === o.value}
          className="ob-button"
          data-variant={choice === o.value ? "primary" : "ghost"}
          style={{ minHeight: 32, padding: "0 10px", fontSize: "var(--text-sm)" }}
          onClick={() => setChoice(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
