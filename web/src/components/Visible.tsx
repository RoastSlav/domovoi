import { useEffect, useId, type ReactNode } from "react";
import { Disclosure } from "./Disclosure";
import { useDetailsScope } from "./DetailsScope";
import { useVisibility, visibilityAttrs } from "./visibilityContext";
import styles from "./visibility.module.css";

/** Props of `Visible`: P0 and P1 cannot be folded, so they have no label, count or reveal. */
export type VisibleProps =
  | { category: "P0" | "P1"; children: ReactNode; id: string }
  | {
      category: "P2" | "P3";
      children: ReactNode;
      count?: number;
      id: string;
      label: string;
      reveal?: boolean;
    };

/**
 * Gives a block of a page its visibility category and draws it as the detail level says:
 * shown in place, behind a labelled toggle (not rendered until opened), or, for metadata in
 * Essential, behind the scope's "+N details" button. P0 and P1 always render in place.
 *
 * `id` names the kind of block (`operation.banner`), never data. Do not put a P0 block inside a
 * P2 or P3 one, and do not use it for dialogs, toasts, menus or tooltips.
 */
export function Visible(props: VisibleProps) {
  const { category, id, children } = props;
  const visibility = useVisibility(category);
  const scope = useDetailsScope();
  const key = useId();
  const folded = props.category === "P2" || props.category === "P3";
  const count = folded ? (props.count ?? 1) : 0;
  const register = scope?.register;
  const hiddenInScope = folded && visibility.hidden && register !== undefined;
  useEffect(() => {
    if (!hiddenInScope || !register) {
      return undefined;
    }
    return register(key, count);
  }, [hiddenInScope, register, key, count]);

  const attrs = visibilityAttrs(category, id);
  if (!folded || visibility.shown) {
    return (
      <div className={styles.contents} {...attrs} data-presentation="shown">
        {children}
      </div>
    );
  }
  if (hiddenInScope) {
    if (!scope?.revealed) {
      return null;
    }
    return (
      <div className={styles.contents} {...attrs} data-presentation="shown" data-revealed="true">
        {children}
      </div>
    );
  }
  // Collapsed, or hidden with no scope to reveal it: reachable, never dropped.
  return (
    <div className={styles.contents} {...attrs} data-presentation="collapsed">
      <Disclosure label={props.label} count={props.count} reveal={props.reveal}>
        {children}
      </Disclosure>
    </div>
  );
}
