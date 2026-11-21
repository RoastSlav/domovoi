import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import styles from "./visibility.module.css";

interface Scope {
  /** Adds a hidden block of `count` details; returns the function that takes it away again. */
  register: (key: string, count: number) => () => void;
  revealed: boolean;
}

/** What `Visible` reads to learn whether a hidden block sits in a scope and is revealed. */
export const DetailsScopeContext = createContext<Scope | null>(null);

/** The scope above, or null when there is none. */
export function useDetailsScope(): Scope | null {
  return useContext(DetailsScopeContext);
}

/**
 * Groups the metadata blocks of one place. Blocks a detail level hides stay out of the page;
 * one "+N details" button after them brings them back and puts them away again.
 *
 * Put the scope directly after the metadata blocks it covers, so the button sits where the
 * facts would be. Without a hidden block it draws nothing.
 */
export function DetailsScope({ children }: { children: ReactNode }) {
  const [blocks, setBlocks] = useState<ReadonlyMap<string, number>>(new Map());
  const [revealed, setRevealed] = useState(false);
  const register = useCallback((key: string, count: number) => {
    setBlocks((now) => new Map(now).set(key, count));
    return () =>
      setBlocks((now) => {
        const next = new Map(now);
        next.delete(key);
        return next;
      });
  }, []);
  const scope = useMemo(() => ({ register, revealed }), [register, revealed]);
  let total = 0;
  blocks.forEach((count) => {
    total += count;
  });
  return (
    <DetailsScopeContext.Provider value={scope}>
      {children}
      {total > 0 && (
        <button
          type="button"
          className={styles.more}
          aria-expanded={revealed}
          onClick={() => setRevealed((now) => !now)}
        >
          {revealed ? `Hide ${total} details` : total === 1 ? "+1 detail" : `+${total} details`}
        </button>
      )}
    </DetailsScopeContext.Provider>
  );
}
