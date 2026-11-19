import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useSlotInternals } from "./RightSlotContext";
import type { SurfaceSpec } from "./types";

/**
 * Registers a side surface with the slot and renders its content into the slot while it is
 * open. A closed surface renders nothing and mounts none of its children. Unmounting while
 * open removes the surface without calling `onClose`.
 */
export function RightSlotSurface({
  spec,
  children,
  actions,
  onClose,
  initialFocus,
}: {
  spec: SurfaceSpec;
  children: ReactNode;
  /** Header actions, left of the close button. */
  actions?: ReactNode;
  /** Called when the slot closes the surface (Escape, close button, tab close); not on unmount. */
  onClose?: (() => void) | undefined;
  /** Where focus goes on opening. Default: the surface's heading. */
  initialFocus?: (() => HTMLElement | null) | undefined;
}) {
  const { api, dispatch, targets, setHandlers, takeFocus } = useSlotInternals();
  const { id, label, icon } = spec;

  useEffect(() => {
    dispatch({ type: "register", spec: { id, label, icon } });
    return () => dispatch({ type: "unregister", id });
  }, [dispatch, id, label, icon]);

  useEffect(() => {
    setHandlers(id, { onClose });
    return () => setHandlers(id, undefined);
  }, [setHandlers, id, onClose]);

  const open = api.isOpen(id);
  const body = targets[id]?.body ?? null;
  const actionsTarget = targets[id]?.actions ?? null;

  useEffect(() => {
    if (!open || !body || !takeFocus(id)) {
      return;
    }
    const heading = body.closest("[data-surface-id]")?.querySelector<HTMLElement>("h2");
    (initialFocus?.() ?? heading)?.focus();
    // initialFocus is read once, at the moment the surface shows.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, body, id, takeFocus]);

  if (!open || !body) {
    return null;
  }
  return (
    <>
      {createPortal(children, body)}
      {actions && actionsTarget ? createPortal(actions, actionsTarget) : null}
    </>
  );
}
