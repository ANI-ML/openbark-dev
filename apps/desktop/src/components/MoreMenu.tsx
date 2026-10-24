import { Ellipsis } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useT } from "../i18n";

/**
 * Review's More (ADR-0027): the actions that aren't the main path, and every
 * destructive one, behind one icon button. A disclosure, not a menu: it
 * holds buttons (and Delete visit's own reason menu). Escape or a click
 * outside closes it and returns focus to the button.
 *
 * `children` gets `close` so an item can shut the panel when it acts.
 */
export function MoreMenu({ children }: { children: (close: () => void) => ReactNode }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panelId = useId();

  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    wrap.current?.querySelector<HTMLElement>(".more-panel button:not(:disabled)")?.focus();
    const onDown = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      // Delete visit's reason menu handles its own Escape first.
      if (e.key === "Escape" && !e.defaultPrevented) close();
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    <div className="more-menu" ref={wrap}>
      <button
        ref={button}
        type="button"
        className="ob-button more-button"
        aria-label={t("More actions")}
        title={t("More actions")}
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((o) => !o)}
      >
        <Ellipsis size={20} aria-hidden />
      </button>
      {open && (
        <div className="more-panel" id={panelId} role="group" aria-label={t("More actions")}>
          {children(() => close(false))}
        </div>
      )}
    </div>
  );
}
