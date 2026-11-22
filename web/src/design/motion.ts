/** The motion tokens a script may read (see `tokens.css`). */
export type MotionName = "fast" | "base" | "panel" | "graph" | "graph-edge" | "layout";

/** No transition runs longer than this, whatever a token says. */
export const MAX_TRANSITION_MS = 600;

/**
 * Parses a CSS duration: "240ms" is 240, "0.24s" is 240. Anything else (empty, "auto", NaN,
 * negative) is 0, and the result never exceeds `MAX_TRANSITION_MS`.
 */
export function parseDuration(text: string): number {
  const match = /^(-?\d*\.?\d+)(ms|s)$/.exec(text.trim());
  if (!match) {
    return 0;
  }
  const value = Number(match[1]) * (match[2] === "s" ? 1000 : 1);
  return Number.isFinite(value) && value > 0 ? Math.min(Math.round(value), MAX_TRANSITION_MS) : 0;
}

/**
 * The token's computed value in milliseconds, read NOW, so a change of the reduced-motion
 * setting applies to the next animation. Reduced motion works because the tokens are 0; 0 means
 * do not animate at all. A missing token is 0.
 */
export function motionMs(name: MotionName, root: HTMLElement = document.documentElement): number {
  return parseDuration(getComputedStyle(root).getPropertyValue(`--motion-${name}`));
}
