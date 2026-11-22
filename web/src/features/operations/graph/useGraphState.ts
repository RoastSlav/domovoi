import { useCallback, useState } from "react";

/** What the person changed about the graph, in memory only: open groups and the window. */
export function useGraphState() {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState(false);

  const toggleGroup = useCallback((id: string) => {
    setExpanded((now) => {
      const next = new Set(now);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });
  }, []);
  const foldAll = useCallback(() => setExpanded(new Set()), []);

  return { expanded, showAll, setShowAll, toggleGroup, foldAll };
}
