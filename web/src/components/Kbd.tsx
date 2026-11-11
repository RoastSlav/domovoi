import type { ReactNode } from "react";
import styles from "./status.module.css";

/** A key on the keyboard, for shortcut hints in tooltips and help text. */
export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className={styles.kbd}>{children}</kbd>;
}
