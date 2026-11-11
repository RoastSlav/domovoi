import type { ReactNode } from "react";
import styles from "./layout.module.css";

/**
 * A run of text that is quieter, smaller or monospace than the text around it: meta lines,
 * timestamps with their words, a command or key in a sentence.
 *
 * Use it in place of a class with a colour or a font size in a page's own stylesheet. Do
 * not use it for a status (`StatusPill`), a label (`Tag`) or a heading (`Section`).
 */
export function Text({
  muted = false,
  mono = false,
  strong = false,
  size,
  block = false,
  children,
}: {
  /** A paragraph of its own, with no margin, in place of a run inside a line. */
  block?: boolean;
  children: ReactNode;
  /** The monospace font, wrapped anywhere: commands, checks, paths, IDs. */
  mono?: boolean;
  /** Secondary text. */
  muted?: boolean;
  size?: "xs" | "sm" | "lg";
  strong?: boolean;
}) {
  const Element = block ? "p" : "span";
  return (
    <Element
      className={styles.text}
      data-muted={muted || undefined}
      data-mono={mono || undefined}
      data-strong={strong || undefined}
      data-size={size}
    >
      {children}
    </Element>
  );
}
