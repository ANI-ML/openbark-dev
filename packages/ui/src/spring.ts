/**
 * A small spring animator for gesture-driven motion (ADR-0010). It starts
 * from the current on-screen value and the finger's velocity, so a drag
 * hands off to the animation with no seam, and it can be re-targeted or
 * stopped at any frame (interruptible). Parameters follow Apple's design
 * vocabulary: damping ratio (1 = no overshoot) and response (seconds).
 */
export interface SpringOptions {
  /** 1.0 = critically damped (default); below 1 overshoots. */
  damping?: number;
  /** Roughly how quickly the value reaches the target, in seconds. */
  response?: number;
  /** Starting velocity, in units per second (e.g. px/s from a drag). */
  velocity?: number;
}

export interface SpringHandle {
  stop: () => void;
}

export function animateSpring(
  from: number,
  to: number,
  onFrame: (value: number) => void,
  { damping = 1, response = 0.35, velocity = 0 }: SpringOptions = {},
  onDone?: () => void,
): SpringHandle {
  const stiffness = (2 * Math.PI / response) ** 2;
  const friction = (4 * Math.PI * damping) / response;
  let x = from - to; // displacement from the target
  let v = velocity;
  let last = performance.now();
  let frame = 0;
  let stopped = false;
  const step = (now: number) => {
    if (stopped) return;
    // Integrate in small fixed steps for stability at any frame rate.
    let dt = Math.min(0.064, (now - last) / 1000);
    last = now;
    while (dt > 0) {
      const h = Math.min(dt, 1 / 240);
      const a = -stiffness * x - friction * v;
      v += a * h;
      x += v * h;
      dt -= h;
    }
    if (Math.abs(x) < 0.5 && Math.abs(v) < 5) {
      onFrame(to);
      onDone?.();
      return;
    }
    onFrame(to + x);
    frame = requestAnimationFrame(step);
  };
  frame = requestAnimationFrame(step);
  return {
    stop: () => {
      stopped = true;
      cancelAnimationFrame(frame);
    },
  };
}

/**
 * Where a flick would come to rest (Apple's projection from scroll
 * deceleration): add this to the release position, then pick the nearest
 * resting point.
 */
export function project(velocity: number, decelerationRate = 0.998): number {
  return ((velocity / 1000) * decelerationRate) / (1 - decelerationRate);
}

/** Resistance past an edge: the further you pull, the less it follows. */
export function rubberband(overshoot: number, dimension: number, constant = 0.55): number {
  return (overshoot * dimension * constant) / (dimension + constant * Math.abs(overshoot));
}

export const prefersReducedMotion = () =>
  typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
