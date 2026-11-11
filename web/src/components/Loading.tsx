import { useEffect, useState } from "react";
import { Spinner } from "./Spinner";
import styles from "./state.module.css";

/** How long a load may take before it is worth saying so. */
export const LOADING_DELAY_MS = 300;

/**
 * "Loading…" for a page or panel, announced politely. It stays silent for the first moment,
 * so a fast load does not flash.
 *
 * Use it where the shape of the content is not known. Where it is known (a table, a card),
 * use `Skeleton` so nothing jumps when the data arrives.
 */
export function Loading({ label = "Loading…" }: { label?: string }) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), LOADING_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);
  return (
    <div className={styles.loading} role="status">
      {shown && (
        <>
          <Spinner />
          {label}
        </>
      )}
    </div>
  );
}
