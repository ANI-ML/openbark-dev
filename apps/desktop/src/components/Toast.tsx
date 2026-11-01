import { Button } from "@openbark/ui";
import { m } from "motion/react";
import { useEffect, useRef, useState } from "react";
import type { ToastAction } from "../context";
import { DUR, EASE_ENTER, EXIT } from "../motion";
import { PHONE, useMediaQuery } from "../useMediaQuery";

/**
 * The toast (DESIGN.md §5: Undo rather than "are you sure" for anything that
 * can be reversed). It closes after `ms`, but not while the pointer or
 * keyboard focus is on it, so there's time to reach its action (Undo); the
 * clock then carries on from where it stopped. A thin ink bar shows the time
 * left and holds with it (design-motion: from Kokonut UI's undo button). It
 * stays a polite status message for screen readers. It rises in from the
 * bottom corner (from the top on phones) and leaves the same way; the app
 * wraps it in AnimatePresence.
 */
export function Toast({
  message,
  action,
  more,
  onClose,
  ms = 4000,
}: {
  message: string;
  action?: ToastAction;
  /** Buttons after `action` (the update notice's Download after What's new). One press closes the toast. */
  more?: ToastAction[];
  onClose: () => void;
  ms?: number;
}) {
  const actions = [...(action ? [action] : []), ...(more ?? [])];
  const [pointer, setPointer] = useState(false);
  const [focus, setFocus] = useState(false);
  const done = useRef(false);
  // The latest onClose, so a re-render of the app doesn't restart the timer.
  const close = useRef(onClose);
  close.current = onClose;
  const held = pointer || focus;
  // Time left, kept across holds so the bar and the clock agree.
  const left = useRef(ms);
  const phone = useMediaQuery(PHONE);
  const from = phone ? -12 : 12;

  useEffect(() => {
    if (held) return;
    const started = Date.now();
    const t = setTimeout(() => close.current(), Math.max(0, left.current));
    return () => {
      clearTimeout(t);
      left.current -= Date.now() - started;
    };
  }, [held]);

  return (
    <m.div
      className="ob-toast"
      data-action={actions.length ? "" : undefined}
      role="status"
      initial={{ opacity: 0, y: from }}
      animate={{ opacity: 1, y: 0, transition: { duration: DUR.standard, ease: EASE_ENTER } }}
      exit={{ opacity: 0, y: from, transition: EXIT }}
      onPointerEnter={() => setPointer(true)}
      onPointerLeave={() => setPointer(false)}
      onFocus={() => setFocus(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocus(false);
      }}
    >
      <span>{message}</span>
      {actions.map((a) => (
        <Button
          key={a.label}
          variant="ghost"
          size="compact"
          className="toast-action"
          onClick={() => {
            if (done.current) return;
            done.current = true;
            a.run();
            close.current();
          }}
        >
          {a.label}
        </Button>
      ))}
      <span
        className="ob-toast-time"
        aria-hidden
        style={{ animationDuration: `${ms}ms`, animationPlayState: held ? "paused" : "running" }}
      />
    </m.div>
  );
}
