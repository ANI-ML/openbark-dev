import { Button } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import type { ToastAction } from "../context";

/**
 * The toast (DESIGN.md §5: Undo rather than "are you sure" for anything that
 * can be reversed). It closes after `ms`, but not while the pointer or
 * keyboard focus is on it, so there's time to reach its action (Undo). It
 * stays a polite status message for screen readers.
 */
export function Toast({
  message,
  action,
  onClose,
  ms = 4000,
}: {
  message: string;
  action?: ToastAction;
  onClose: () => void;
  ms?: number;
}) {
  const [pointer, setPointer] = useState(false);
  const [focus, setFocus] = useState(false);
  const done = useRef(false);
  // The latest onClose, so a re-render of the app doesn't restart the timer.
  const close = useRef(onClose);
  close.current = onClose;
  const held = pointer || focus;

  useEffect(() => {
    if (held) return;
    const t = setTimeout(() => close.current(), ms);
    return () => clearTimeout(t);
  }, [held, ms]);

  return (
    <div
      className="ob-toast"
      data-action={action ? "" : undefined}
      role="status"
      onPointerEnter={() => setPointer(true)}
      onPointerLeave={() => setPointer(false)}
      onFocus={() => setFocus(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocus(false);
      }}
    >
      <span>{message}</span>
      {action && (
        <Button
          variant="ghost"
          size="compact"
          className="toast-action"
          onClick={() => {
            if (done.current) return;
            done.current = true;
            action.run();
            close.current();
          }}
        >
          {action.label}
        </Button>
      )}
    </div>
  );
}
