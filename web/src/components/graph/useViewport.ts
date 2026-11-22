import { useEffect, useMemo, useRef, type RefObject } from "react";
import { motionMs } from "../../design/motion";
import { createAnimator, easeOut } from "./animate";
import {
  fitView,
  lerpView,
  panView,
  revealView,
  viewBoxOf,
  zoomView,
  type Rect,
  type Size,
  type View,
} from "./viewport";

/** What the frame measures as in jsdom, which has no layout. */
const FALLBACK: Size = { w: 960, h: 480 };
/** A press that moves less than this many pixels is a click, not a drag. */
const DRAG_SLOP = 4;
/** One notch of ctrl or cmd + wheel. */
const WHEEL_STEP = 1.15;

/** What the engine asks of the view: fit, zoom, reveal; pan and wheel are handled inside. */
export interface Viewport {
  /** A new graph size: refits unless the person has moved the view since the last fit. */
  setGraph(size: { width: number; height: number }, animate: boolean): void;
  fit(): void;
  zoomBy(factor: number): void;
  reveal(rect: Rect): void;
}

/**
 * Owns the SVG `viewBox` attribute: pan by dragging the background, zoom with ctrl or cmd +
 * wheel, fit, zoom and reveal for the handle. Tweens run on `requestAnimationFrame` over
 * `motionMs("layout")`, read when they start, and only when `animateLayout` is set. A plain
 * wheel is never captured.
 */
export function useViewport(
  svgRef: RefObject<SVGSVGElement | null>,
  animateLayout: boolean,
  onBackground: () => void,
): Viewport {
  const state = useRef({
    view: { cx: 0, cy: 0, scale: 1 } as View,
    graph: { width: 0, height: 0 },
    moved: false,
    animate: animateLayout,
    background: onBackground,
  });
  useEffect(() => {
    state.current.animate = animateLayout;
    state.current.background = onBackground;
  });
  const animator = useMemo(() => createAnimator(), []);

  const api = useMemo(() => {
    const s = state.current;
    const size = (): Size => {
      const box = svgRef.current?.getBoundingClientRect();
      return box && box.width > 0 && box.height > 0 ? { w: box.width, h: box.height } : FALLBACK;
    };
    const write = (view: View): void => {
      s.view = view;
      svgRef.current?.setAttribute("viewBox", viewBoxOf(view, size()));
    };
    const go = (target: View, animate: boolean): void => {
      const from = s.view;
      animator.run(animate && s.animate ? motionMs("layout") : 0, (k) =>
        write(lerpView(from, target, easeOut(k))),
      );
    };
    const fit = (animate: boolean): void => {
      s.moved = false;
      go(fitView(s.graph, size()), animate);
    };
    const zoom = (factor: number, anchor: { x: number; y: number } | null): void => {
      animator.cancel();
      s.moved = true;
      write(zoomView(s.view, factor, anchor));
    };
    const pan = (dx: number, dy: number): void => {
      animator.cancel();
      s.moved = true;
      write(panView(s.view, dx, dy));
    };
    const viewport: Viewport = {
      setGraph(graph, animate) {
        s.graph = graph;
        if (!s.moved) {
          fit(animate);
        }
      },
      fit: () => fit(true),
      zoomBy: (factor) => zoom(factor, null),
      reveal(rect) {
        const next = revealView(s.view, size(), rect);
        if (next !== s.view) {
          go(next, true);
        }
      },
    };
    return { viewport, size, write, zoom, pan, refit: () => fit(false) };
  }, [svgRef, animator]);

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) {
      return;
    }
    const s = state.current;
    let drag: { x: number; y: number; travelled: number } | null = null;
    const onWheel = (event: WheelEvent): void => {
      if (!event.ctrlKey && !event.metaKey) {
        return;
      }
      event.preventDefault();
      const box = svg.getBoundingClientRect();
      api.zoom(event.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP, {
        x: event.clientX - box.left - box.width / 2,
        y: event.clientY - box.top - box.height / 2,
      });
    };
    const onDown = (event: PointerEvent): void => {
      if (event.button !== 0 || (event.target as Element).closest("[data-node-id]")) {
        return;
      }
      drag = { x: event.clientX, y: event.clientY, travelled: 0 };
      svg.setPointerCapture?.(event.pointerId);
      svg.setAttribute("data-panning", "true");
    };
    const onMove = (event: PointerEvent): void => {
      if (!drag) {
        return;
      }
      const dx = event.clientX - drag.x;
      const dy = event.clientY - drag.y;
      drag = {
        x: event.clientX,
        y: event.clientY,
        travelled: drag.travelled + Math.abs(dx) + Math.abs(dy),
      };
      if (drag.travelled >= DRAG_SLOP) {
        api.pan(dx, dy);
      }
    };
    const onUp = (event: PointerEvent): void => {
      if (!drag) {
        return;
      }
      const click = event.type === "pointerup" && drag.travelled < DRAG_SLOP;
      drag = null;
      svg.removeAttribute("data-panning");
      if (click) {
        s.background();
      }
    };
    const onResize = (): void => (s.moved ? api.write(s.view) : api.refit());
    svg.addEventListener("wheel", onWheel, { passive: false });
    svg.addEventListener("pointerdown", onDown);
    svg.addEventListener("pointermove", onMove);
    svg.addEventListener("pointerup", onUp);
    svg.addEventListener("pointercancel", onUp);
    window.addEventListener("resize", onResize);
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(onResize) : null;
    observer?.observe(svg);
    return () => {
      animator.cancel();
      svg.removeEventListener("wheel", onWheel);
      svg.removeEventListener("pointerdown", onDown);
      svg.removeEventListener("pointermove", onMove);
      svg.removeEventListener("pointerup", onUp);
      svg.removeEventListener("pointercancel", onUp);
      window.removeEventListener("resize", onResize);
      observer?.disconnect();
    };
  }, [svgRef, api, animator]);

  return api.viewport;
}
