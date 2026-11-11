import { useLayoutEffect, useState, type CSSProperties, type RefObject } from "react";

/** Which edge of its anchor a floating element prefers. */
export type Side = "top" | "bottom" | "left" | "right";

/** A box on screen: what `getBoundingClientRect` returns, reduced to what placement needs. */
export interface Box {
  height: number;
  left: number;
  top: number;
  width: number;
}

const GAP = 8;
const MARGIN = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

function at(anchor: Box, floating: Box, side: Side): { left: number; top: number } {
  const centreX = anchor.left + anchor.width / 2 - floating.width / 2;
  const centreY = anchor.top + anchor.height / 2 - floating.height / 2;
  switch (side) {
    case "top":
      return { left: centreX, top: anchor.top - floating.height - GAP };
    case "bottom":
      return { left: centreX, top: anchor.top + anchor.height + GAP };
    case "left":
      return { left: anchor.left - floating.width - GAP, top: centreY };
    case "right":
      return { left: anchor.left + anchor.width + GAP, top: centreY };
  }
}

const OPPOSITE: Record<Side, Side> = { top: "bottom", bottom: "top", left: "right", right: "left" };

/**
 * Where a floating element of size `floating` goes beside `anchor`: on `side` if it fits in
 * the viewport, on the opposite side if only that fits, and always pulled back on screen.
 */
export function place(
  anchor: Box,
  floating: Box,
  side: Side,
  viewport: { height: number; width: number },
): { left: number; side: Side; top: number } {
  const fits = (candidate: Side): boolean => {
    const { left, top } = at(anchor, floating, candidate);
    return candidate === "top" || candidate === "bottom"
      ? top >= MARGIN && top + floating.height <= viewport.height - MARGIN
      : left >= MARGIN && left + floating.width <= viewport.width - MARGIN;
  };
  const chosen = fits(side) || !fits(OPPOSITE[side]) ? side : OPPOSITE[side];
  const { left, top } = at(anchor, floating, chosen);
  return {
    side: chosen,
    left: clamp(left, MARGIN, viewport.width - floating.width - MARGIN),
    top: clamp(top, MARGIN, viewport.height - floating.height - MARGIN),
  };
}

/** Where floating content is mounted: inside an open modal dialog if the anchor is in one. */
export function floatingRoot(anchor: Element | null): Element {
  return anchor?.closest("dialog") ?? document.body;
}

/**
 * The fixed position of `floating` beside `anchor` while `open`, kept right as the page
 * scrolls or resizes. Hidden until first measured, so it never flashes in a corner.
 */
export function useFloating(
  anchor: RefObject<Element | null>,
  floating: RefObject<HTMLElement | null>,
  open: boolean,
  side: Side,
): CSSProperties {
  const [style, setStyle] = useState<CSSProperties>({ visibility: "hidden" });
  useLayoutEffect(() => {
    if (!open) {
      return undefined;
    }
    const update = (): void => {
      // An anchor that is `display: contents` has no box of its own: measure what it wraps.
      let target: Element | null = anchor.current;
      while (target && getComputedStyle(target).display === "contents") {
        target = target.firstElementChild;
      }
      if (!target || !floating.current) {
        return;
      }
      const placed = place(
        target.getBoundingClientRect(),
        floating.current.getBoundingClientRect(),
        side,
        { width: window.innerWidth, height: window.innerHeight },
      );
      setStyle({ left: placed.left, top: placed.top });
    };
    update();
    window.addEventListener("scroll", update, true);
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("scroll", update, true);
      window.removeEventListener("resize", update);
    };
  }, [anchor, floating, open, side]);
  return style;
}
