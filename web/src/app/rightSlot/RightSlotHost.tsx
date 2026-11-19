import { useCallback, useEffect, useRef } from "react";
import { SegmentedControl, SidePanel } from "../../components";
import { useSlotInternals } from "./RightSlotContext";
import styles from "./rightSlot.module.css";
import type { SurfaceId } from "./types";

/** One visible surface: an empty panel that its surface fills through portals. */
function Panel({ id, mode }: { id: SurfaceId; mode: "side" | "sheet" }) {
  const { api, setTarget } = useSlotInternals();
  const body = useCallback(
    (element: HTMLElement | null) => {
      setTarget(id, "body", element);
    },
    [id, setTarget],
  );
  const actions = useCallback(
    (element: HTMLElement | null) => {
      setTarget(id, "actions", element);
    },
    [id, setTarget],
  );
  const spec = api.specs[id];
  if (!spec) {
    return null;
  }
  return (
    <section className={styles.panel} data-surface-id={id} aria-label={spec.label}>
      <SidePanel
        title={spec.label}
        mode={mode}
        onClose={() => api.close(id)}
        actions={<span ref={actions} className={styles.actions} />}
      >
        <div ref={body} className={styles.content} />
      </SidePanel>
    </section>
  );
}

/** The id of the surface that holds `element`, if any. */
function surfaceOf(element: Element | null): string | null {
  return element?.closest("[data-surface-id]")?.getAttribute("data-surface-id") ?? null;
}

/**
 * Draws the open surfaces: beside the page, or as a modal sheet when there is no room. Sits
 * once in the shell, as a sibling of `<main>` inside the page body whose width it measures.
 */
export function RightSlotHost() {
  const { api, layout, dispatch } = useSlotInternals();
  const { close } = api;
  const root = useRef<HTMLElement>(null);
  const { mode, visible, tabs } = layout;

  // The page body is the host's parent: main plus slot, a width that does not depend on the slot.
  useEffect(() => {
    const parent = root.current?.parentElement;
    if (!parent) {
      return undefined;
    }
    const measure = () => dispatch({ type: "measure", available: parent.clientWidth });
    if (typeof ResizeObserver === "function") {
      const observer = new ResizeObserver((entries) => {
        const width = entries[0]?.contentRect.width;
        dispatch({ type: "measure", available: width ?? parent.clientWidth });
      });
      observer.observe(parent);
      return () => observer.disconnect();
    }
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [dispatch]);

  // A native listener: a surface's content is portalled in from outside this component's React
  // tree, so React's synthetic keydown would never reach the host. One thing per press: tips,
  // menus and dialogs have already taken their own Escape (and prevented it).
  useEffect(() => {
    const element = root.current;
    if (!element) {
      return undefined;
    }
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) {
        return;
      }
      const id = surfaceOf(document.activeElement);
      if (id) {
        event.preventDefault();
        close(id);
      }
    };
    element.addEventListener("keydown", onKeyDown);
    return () => element.removeEventListener("keydown", onKeyDown);
  }, [close]);

  useEffect(() => {
    if (mode !== "sheet") {
      return undefined;
    }
    // Only what the sheet made inert is lifted again: an element already inert stays so.
    const inert = Array.from(document.querySelectorAll("[data-slot-inert]")).filter(
      (element) => !element.hasAttribute("inert"),
    );
    inert.forEach((element) => element.setAttribute("inert", ""));
    return () => inert.forEach((element) => element.removeAttribute("inert"));
  }, [mode]);

  const newest = visible[visible.length - 1] ?? "";
  const label = visible.length > 1 ? "Side panels" : (api.specs[newest]?.label ?? "Side panel");
  const sheet = mode === "sheet";
  return (
    <aside
      ref={root}
      className={styles.slot}
      hidden={mode === "closed"}
      data-mode={mode}
      data-count={visible.length}
      aria-label={label}
      {...(sheet ? { role: "dialog", "aria-modal": true } : {})}
    >
      {tabs.length > 0 && (
        <div className={styles.tabs}>
          <SegmentedControl
            label="Side panels"
            value={newest}
            onChange={(id) => api.open(id)}
            options={tabs.map((id) => ({
              value: id,
              label: api.specs[id]?.label ?? id,
              icon: api.specs[id]?.icon ?? "info",
            }))}
          />
        </div>
      )}
      <div className={styles.panels}>
        {visible.map((id) => (
          <Panel key={id} id={id} mode={sheet ? "sheet" : "side"} />
        ))}
      </div>
    </aside>
  );
}
