/** What the person sees: the graph point at the centre of the frame and pixels per layout unit. */
export interface View {
  cx: number;
  cy: number;
  scale: number;
}

/** The frame's size in pixels. */
export interface Size {
  w: number;
  h: number;
}

/** A rectangle in layout units. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const MIN_SCALE = 0.25;
export const MAX_SCALE = 2;
/** Space kept around the graph when it is fitted, in layout units (`--space-5`). */
export const FIT_PAD = 24;
/** Space kept around a revealed node (`--space-5`). */
export const REVEAL_MARGIN = 24;

/** Keeps a zoom factor inside 25 % .. 200 %. */
export function clampScale(scale: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
}

/** The `viewBox` attribute that shows `view` in a frame of `size` without distortion. */
export function viewBoxOf(view: View, size: Size): string {
  const w = size.w / view.scale;
  const h = size.h / view.scale;
  return `${view.cx - w / 2} ${view.cy - h / 2} ${w} ${h}`;
}

/** Shows the whole graph, never above 100 %, centred. */
export function fitView(graph: { width: number; height: number }, size: Size): View {
  const fit = Math.min(
    size.w / (graph.width + 2 * FIT_PAD),
    size.h / (graph.height + 2 * FIT_PAD),
    1,
  );
  return { cx: graph.width / 2, cy: graph.height / 2, scale: clampScale(fit) };
}

/** Zooms by `factor` around `anchor` (frame pixels from the centre), or the centre. */
export function zoomView(view: View, factor: number, anchor: { x: number; y: number } | null) {
  const scale = clampScale(view.scale * factor);
  if (!anchor) {
    return { ...view, scale };
  }
  const gx = view.cx + anchor.x / view.scale;
  const gy = view.cy + anchor.y / view.scale;
  return { cx: gx - anchor.x / scale, cy: gy - anchor.y / scale, scale };
}

/** Moves the picture by pixels, as if dragged: the graph follows the pointer. */
export function panView(view: View, dx: number, dy: number): View {
  return { ...view, cx: view.cx - dx / view.scale, cy: view.cy - dy / view.scale };
}

/** The smallest pan that puts `rect` inside the frame with a margin; the view itself when it already is. */
export function revealView(view: View, size: Size, rect: Rect): View {
  const axis = (centre: number, extent: number, start: number, length: number): number => {
    const half = extent / view.scale / 2;
    const margin = REVEAL_MARGIN / view.scale;
    const lo = centre - half + margin;
    const hi = centre + half - margin;
    if (length >= hi - lo) {
      return start + length / 2;
    }
    if (start < lo) {
      return start - margin + half;
    }
    if (start + length > hi) {
      return start + length + margin - half;
    }
    return centre;
  };
  const cx = axis(view.cx, size.w, rect.x, rect.w);
  const cy = axis(view.cy, size.h, rect.y, rect.h);
  return cx === view.cx && cy === view.cy ? view : { ...view, cx, cy };
}

/** A view between two others. */
export function lerpView(a: View, b: View, k: number): View {
  return {
    cx: a.cx + (b.cx - a.cx) * k,
    cy: a.cy + (b.cy - a.cy) * k,
    scale: a.scale + (b.scale - a.scale) * k,
  };
}
