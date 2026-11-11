import styles from "./state.module.css";

/**
 * A grey placeholder in the shape of content that is loading, so the layout does not shift
 * when it arrives: `line` for text, `block` for a card or panel, `rows` for a list or table.
 * It is hidden from assistive technology; pair it with a `Loading` if nothing else says the
 * view is loading (`QueryBoundary` does this for you).
 */
export function Skeleton({
  shape = "line",
  count = 3,
}: {
  /** How many lines or rows. */
  count?: number;
  shape?: "line" | "block" | "rows";
}) {
  if (shape === "block") {
    return <span className={`${styles.skeleton} ${styles.block}`} aria-hidden="true" />;
  }
  const item = shape === "rows" ? styles.rowShape : styles.line;
  return (
    <span className={shape === "rows" ? styles.rows : undefined} aria-hidden="true">
      {Array.from({ length: count }, (_, index) => (
        <span key={index} className={`${styles.skeleton} ${item}`} />
      ))}
    </span>
  );
}
