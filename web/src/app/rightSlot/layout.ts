import type { SurfaceId } from "./types";

/** Width of one surface in the slot; equals `--right-slot-width` (25rem). */
export const SLOT_WIDTH_PX = 400;
/** The page keeps at least this much; equals `--right-slot-content-min` (40rem). */
export const SLOT_CONTENT_MIN_PX = 640;

/** How the slot is drawn: not at all, beside the page, or as a modal sheet over it. */
export type SlotMode = "closed" | "side" | "sheet";

/** The decision of `slotLayout`. */
export interface SlotLayout {
  mode: SlotMode;
  /** Rendered now, in open order; at most two. */
  visible: readonly SurfaceId[];
  /** When more are open than visible: every open id, to switch between; else empty. */
  tabs: readonly SurfaceId[];
}

/**
 * Decides where open surfaces go. `available` is the width of the page body (the main column
 * plus the slot, in px); `wide` is the desktop media query.
 */
export function slotLayout(input: {
  available: number;
  wide: boolean;
  open: readonly SurfaceId[];
}): SlotLayout {
  const { available, wide, open } = input;
  if (open.length === 0) {
    return { mode: "closed", visible: [], tabs: [] };
  }
  const newest = open.slice(-1);
  const tabsFor = open.length > 1 ? open : [];
  if (!wide) {
    return { mode: "sheet", visible: newest, tabs: tabsFor };
  }
  const fits = (n: number) => available - n * SLOT_WIDTH_PX >= SLOT_CONTENT_MIN_PX;
  const n = Math.min(open.length, 2);
  if (fits(n)) {
    return { mode: "side", visible: open.slice(-n), tabs: [] };
  }
  if (open.length > 1 && fits(1)) {
    return { mode: "side", visible: newest, tabs: open };
  }
  if (fits(1)) {
    return { mode: "side", visible: newest, tabs: [] };
  }
  return { mode: "sheet", visible: newest, tabs: tabsFor };
}
