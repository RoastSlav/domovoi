import { Visible } from "../Visible";
import { edgeStyle } from "./edgeStyle";
import styles from "./graph.module.css";
import type { GraphEdgeModel } from "./types";

/** The edge key: one row per edge kind present, a line sample in its dash pattern and its label. */
export function GraphLegend({ edges }: { edges: readonly GraphEdgeModel[] }) {
  const kinds = new Map<string, string>();
  for (const edge of edges) {
    if (!kinds.has(edge.kind)) {
      kinds.set(edge.kind, edge.label);
    }
  }
  if (kinds.size === 0) {
    return null;
  }
  return (
    <Visible category="P2" id="operation.graph-key" label="Edge key" count={kinds.size}>
      <ul className={styles.key} aria-label="Edge key">
        {[...kinds].map(([kind, label]) => {
          const style = edgeStyle(kind);
          return (
            <li key={kind} className={styles.keyItem}>
              <svg width={32} height={8} viewBox="0 0 32 8" aria-hidden="true" focusable="false">
                <line
                  className={styles.keyLine}
                  x1={1}
                  y1={4}
                  x2={31}
                  y2={4}
                  strokeWidth={style.weight}
                  strokeDasharray={style.dash}
                />
              </svg>
              <span>{label}</span>
            </li>
          );
        })}
      </ul>
    </Visible>
  );
}
