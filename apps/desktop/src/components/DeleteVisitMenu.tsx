import { Button } from "@openbark/ui";
import { Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { REASON_KINDS, reasonLabel } from "../deleting";
import type { ReasonKind } from "../engine";
import { useT } from "../i18n";

/**
 * Review's Delete visit (ADR-0022 section 11): a destructive button that
 * opens a short menu of reasons. Choosing one deletes the visit at once; it
 * waits in Deleted visits for 30 days and the toast offers Undo, so there's
 * no "are you sure". Arrow keys move through the reasons; Escape closes the
 * menu and returns focus to the button.
 *
 * `disabledReason`: the button is shown but can't be used (a signed visit),
 * and this says why in its tooltip.
 */
export function DeleteVisitMenu({
  others,
  busy = false,
  disabledReason,
  onDelete,
}: {
  /** The other animals in a multi-pet visit, already joined ("Olive and Pickles"). */
  others?: string;
  busy?: boolean;
  disabledReason?: string | null;
  onDelete: (kind: ReasonKind) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const items = useRef<(HTMLButtonElement | null)[]>([]);
  const titleId = useId();
  const reasonId = useId();

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) wrap.current?.querySelector<HTMLButtonElement>(":scope > .ob-button")?.focus();
  };

  useEffect(() => {
    if (!open) return;
    items.current[0]?.focus();
    const onDown = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    return () => window.removeEventListener("pointerdown", onDown);
  }, [open]);

  const onKey = (e: KeyboardEvent) => {
    const list = items.current.filter((x): x is HTMLButtonElement => x != null);
    const at = list.findIndex((x) => x === document.activeElement);
    const move = (to: number) => {
      e.preventDefault();
      list[(to + list.length) % list.length]?.focus();
    };
    if (e.key === "ArrowDown") move(at + 1);
    else if (e.key === "ArrowUp") move(at - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(list.length - 1);
    else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === "Tab") setOpen(false);
  };

  if (disabledReason) {
    return (
      <span className="delete-visit" title={disabledReason}>
        <Button variant="danger" size="compact" disabled aria-describedby={reasonId}>
          <Trash2 size={16} aria-hidden />
          {t("Delete visit")}
        </Button>
        <span id={reasonId} className="sr-only">
          {disabledReason}
        </span>
      </span>
    );
  }

  return (
    <div className="delete-visit" ref={wrap}>
      <Button
        variant="danger"
        size="compact"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen((o) => !o)}
      >
        <Trash2 size={16} aria-hidden />
        {t("Delete visit")}
      </Button>
      {open && (
        <div className="delete-menu" onKeyDown={onKey}>
          <p id={titleId} className="label">
            {t("Why delete this visit?")}
          </p>
          {others && (
            <p className="small muted">{t("This visit also has notes for {pets}. They're deleted with it.", { pets: others })}</p>
          )}
          <ul role="menu" aria-labelledby={titleId} className="delete-menu-items">
            {REASON_KINDS.map((kind, i) => (
              <li key={kind} role="none">
                <button
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  className="delete-menu-item"
                  ref={(el) => {
                    items.current[i] = el;
                  }}
                  disabled={busy}
                  onClick={() => {
                    close(true);
                    onDelete(kind);
                  }}
                >
                  {reasonLabel(t, kind)}
                </button>
              </li>
            ))}
          </ul>
          <p className="small muted">{t("Filed under the wrong patient? Use Edit details instead.")}</p>
          <p className="small faint">{t("It moves to Deleted visits for 30 days.")}</p>
        </div>
      )}
    </div>
  );
}
