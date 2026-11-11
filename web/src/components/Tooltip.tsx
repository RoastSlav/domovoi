import {
  cloneElement,
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactElement,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { floatingRoot, useFloating, type Side } from "./floating";
import styles from "./overlay.module.css";

/** How long the pointer rests on something before its tooltip shows. */
export const TOOLTIP_DELAY_MS = 400;

/**
 * A short explanation shown on hover (after a moment) and on keyboard focus (at once), and
 * closed by Escape. It describes its child for assistive technology (`aria-describedby` while
 * open, `aria-description` while closed). It never shows for
 * touch, so anything that matters must also be visible text or an `InfoTip`.
 *
 * Use it for icon-only buttons, statuses, truncated values, exact times and disabled
 * controls. Do not put anything interactive or essential in it.
 */
export function Tooltip({
  content,
  children,
  side = "top",
  describe = true,
}: {
  /** One element: the thing being explained. */
  children: ReactElement<{
    "aria-describedby"?: string | undefined;
    "aria-description"?: string | undefined;
  }>;
  /** The explanation; nothing is shown while it is empty. */
  content: ReactNode;
  /** `false` when the content repeats the child's accessible name (an icon button's label). */
  describe?: boolean;
  side?: Side;
}) {
  const id = useId();
  // Where the bubble is mounted while open; `null` means closed.
  const [root, setRoot] = useState<Element | null>(null);
  const open = root !== null;
  const anchor = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pressed = useRef(false);
  const style = useFloating(anchor, bubble, open, side);
  const empty = content === null || content === undefined || content === "" || content === false;

  useEffect(() => () => clearTimeout(timer.current), []);

  const show = (): void => {
    if (!empty) {
      setRoot(floatingRoot(anchor.current));
    }
  };
  const hide = (): void => {
    clearTimeout(timer.current);
    setRoot(null);
  };
  const onPointerEnter = (event: PointerEvent): void => {
    if (event.pointerType === "touch") {
      return;
    }
    clearTimeout(timer.current);
    timer.current = setTimeout(show, TOOLTIP_DELAY_MS);
  };
  const onPointerDown = (): void => {
    // A click or tap focuses the control; only keyboard focus should open the tooltip.
    pressed.current = true;
    hide();
  };
  const onFocus = (): void => {
    if (!pressed.current) {
      show();
    }
    pressed.current = false;
  };
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape" && open) {
      // Closes only the tooltip, not the dialog it may sit in.
      event.preventDefault();
      event.stopPropagation();
      hide();
    }
  };

  // Open, the child is described by the bubble. Closed, plain text rides on the child as
  // `aria-description`, so the page's text is not doubled; rich content keeps a hidden copy.
  const plain = typeof content === "string" || typeof content === "number";
  const linked = describe && !empty && (open || !plain);
  const described = linked
    ? [children.props["aria-describedby"], id].filter(Boolean).join(" ")
    : children.props["aria-describedby"];
  const description = describe && !empty && !open && plain ? String(content) : undefined;
  return (
    <span
      ref={anchor}
      className={styles.anchor}
      onPointerEnter={onPointerEnter}
      onPointerLeave={hide}
      onPointerDown={onPointerDown}
      onPointerUp={() => {
        pressed.current = false;
      }}
      onFocus={onFocus}
      onBlur={hide}
      onKeyDown={onKeyDown}
    >
      {cloneElement(children, {
        "aria-describedby": described,
        ...(description ? { "aria-description": description } : {}),
      })}
      {root && !empty
        ? createPortal(
            <span ref={bubble} id={id} role="tooltip" className={styles.tooltip} style={style}>
              {content}
            </span>,
            root,
          )
        : linked && (
            <span id={id} hidden>
              {content}
            </span>
          )}
    </span>
  );
}
