import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { animateSpring, prefersReducedMotion, project, rubberband, type SpringHandle } from "./spring";

/** Close the surrounding sheet or dialog with its exit motion. */
const CloseContext = createContext<() => void>(() => {});
export const useSheetClose = () => useContext(CloseContext);

/** How long the exit motion may take before the sheet unmounts anyway (§5.4: large, 320 ms). */
const EXIT_MS = 320;

function useExit(onClose: () => void) {
  const [closing, setClosing] = useState(false);
  const done = useRef(false);
  const close = useCallback(() => {
    if (done.current) return;
    done.current = true;
    setClosing(true);
    // Reduced motion cuts every duration to zero (§5.4).
    setTimeout(onClose, prefersReducedMotion() ? 0 : EXIT_MS);
  }, [onClose]);
  return { closing, close };
}

export interface SheetProps {
  /** Accessible name. */
  label: string;
  onClose: () => void;
  /** False while work is running that closing would interrupt. */
  dismissible?: boolean;
  className?: string;
  /** Content, or a function given `close` (which plays the exit motion first). */
  children: ReactNode | ((close: () => void) => ReactNode);
}

/**
 * DESIGN.md §5.3 level-2 sheet: a solid surface with a border, radius
 * --r-xl, that slides in from the right (bottom on phones) over 320 ms and
 * leaves the same way. No glass (§3.5). On a
 * phone the grabber can be dragged: the sheet follows the finger 1:1,
 * resists upwards, and a flick down (by projected momentum, not just
 * distance) dismisses it; otherwise it springs back with the finger's
 * velocity.
 */
export function Sheet({ label, onClose, dismissible = true, className, children }: SheetProps) {
  const { closing, close } = useExit(onClose);
  const ref = useRef<HTMLElement>(null);
  const drag = useRef<{ start: number; y: number; history: { y: number; t: number }[] } | null>(null);
  const anim = useRef<SpringHandle | null>(null);
  const tryClose = useCallback(() => dismissible && close(), [dismissible, close]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && tryClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tryClose]);

  const setY = (y: number) => {
    if (ref.current) ref.current.style.transform = y ? `translateY(${y}px)` : "";
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (!dismissible) return;
    anim.current?.stop(); // grab it mid-flight: start from where it is now
    const current = ref.current ? new DOMMatrixReadOnly(getComputedStyle(ref.current).transform).m42 : 0;
    (e.target as Element).setPointerCapture(e.pointerId);
    if (ref.current) ref.current.style.transition = "none";
    drag.current = { start: e.clientY - current, y: current, history: [{ y: current, t: e.timeStamp }] };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d || !ref.current) return;
    const raw = e.clientY - d.start;
    const y = raw < 0 ? rubberband(raw, ref.current.offsetHeight) : raw;
    d.y = y;
    d.history.push({ y, t: e.timeStamp });
    if (d.history.length > 6) d.history.shift();
    setY(y);
  };
  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (!d || !ref.current) return;
    const first = d.history[0] ?? { y: d.y, t: 0 };
    const lastPoint = d.history[d.history.length - 1] ?? first;
    const dt = Math.max(1, lastPoint.t - first.t) / 1000;
    const velocity = (lastPoint.y - first.y) / dt; // px/s, positive = down
    const height = ref.current.offsetHeight;
    const resting = d.y + project(velocity);
    if (resting > height * 0.5) {
      // Dismiss: carry the flick into the exit; the backdrop fades as usual.
      close();
      anim.current = animateSpring(d.y, height, setY, { velocity, response: 0.3 });
    } else {
      anim.current = animateSpring(d.y, 0, setY, { velocity, response: 0.35 }, () => {
        if (ref.current) ref.current.style.transition = "";
      });
    }
  };

  return (
    <CloseContext.Provider value={close}>
      <div className="ob-backdrop" data-closing={closing || undefined} onClick={tryClose} />
      <aside
        ref={ref}
        className={className ? `ob-sheet ${className}` : "ob-sheet"}
        data-closing={closing || undefined}
        role="dialog"
        aria-modal="true"
        aria-label={label}
      >
        <button
          type="button"
          className="ob-grabber"
          aria-hidden
          tabIndex={-1}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        />
        {typeof children === "function" ? children(tryClose) : children}
      </aside>
    </CloseContext.Provider>
  );
}

/**
 * A centred dialog for one-time notices (consent, who is using OpenBark): a
 * solid level-2 surface over a dimmed screen.
 */
export function Dialog({
  labelledBy,
  onClose,
  dismissible = Boolean(onClose),
  className,
  children,
}: {
  labelledBy: string;
  /** Called after the exit motion. */
  onClose?: () => void;
  /** Backdrop click and Escape close it (default: when onClose is given). */
  dismissible?: boolean;
  className?: string;
  children: ReactNode | ((close: () => void) => ReactNode);
}) {
  const { closing, close } = useExit(onClose ?? (() => {}));
  useEffect(() => {
    if (!dismissible) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismissible, close]);
  return (
    <CloseContext.Provider value={close}>
      <div className="ob-backdrop" data-closing={closing || undefined} onClick={dismissible ? close : undefined} />
      <div
        className={className ? `dialog ${className}` : "dialog"}
        data-closing={closing || undefined}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
      >
        {typeof children === "function" ? children(close) : children}
      </div>
    </CloseContext.Provider>
  );
}
