import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { floatingRoot, useFloating, type Side } from "./floating";
import styles from "./overlay.module.css";

/**
 * A small floating panel beside the control that opened it. It takes focus when it opens,
 * closes on Escape, on a click outside and when focus leaves it, and hands focus back to
 * its anchor.
 *
 * Use it through `InfoTip` and `Menu`. Do not use it for anything that must be answered:
 * that is a `Dialog`.
 */
export function Popover({
  anchor,
  children,
  id,
  label,
  onClose,
  role = "dialog",
  side = "bottom",
}: {
  /** The control the panel belongs to; focus returns to it. */
  anchor: RefObject<HTMLElement | null>;
  children: ReactNode;
  id?: string;
  /** The panel's accessible name. */
  label: string;
  onClose: () => void;
  role?: "dialog" | "menu";
  side?: Side;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const style = useFloating(anchor, panel, true, side);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const opener = anchor.current;
    if (role === "dialog") {
      panel.current?.focus();
    }
    const onPointerDown = (event: PointerEvent): void => {
      const target = event.target as Node | null;
      if (target && !panel.current?.contains(target) && !opener?.contains(target)) {
        close.current();
      }
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      // Hand focus back only if it was still inside; a click elsewhere keeps its own focus.
      if (!document.activeElement || document.activeElement === document.body) {
        opener?.focus();
      }
    };
  }, [anchor, role]);

  return createPortal(
    <div
      ref={panel}
      id={id}
      role={role}
      aria-label={label}
      tabIndex={-1}
      className={styles.popover}
      style={style}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          anchor.current?.focus();
          onClose();
        }
      }}
      onBlur={(event) => {
        const next = event.relatedTarget as Node | null;
        if (next && !panel.current?.contains(next) && !anchor.current?.contains(next)) {
          onClose();
        }
      }}
    >
      {children}
    </div>,
    floatingRoot(anchor.current),
  );
}
