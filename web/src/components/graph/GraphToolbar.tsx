import { Button } from "../Button";
import { IconButton } from "../IconButton";
import styles from "./graph.module.css";

/** Fit to view, zoom and, when a group is open, "Fold technical steps": ordinary tab stops before the graph. */
export function GraphToolbar({
  onFit,
  onZoom,
  onFoldAll,
}: {
  onFit(): void;
  /** A zoom factor: above 1 zooms in. */
  onZoom(factor: number): void;
  onFoldAll?: (() => void) | undefined;
}) {
  return (
    <div role="toolbar" aria-label="Graph controls" className={styles.toolbar}>
      <IconButton icon="fit" label="Fit to view" onClick={onFit} />
      <IconButton icon="zoom-in" label="Zoom in" onClick={() => onZoom(ZOOM_IN)} />
      <IconButton icon="zoom-out" label="Zoom out" onClick={() => onZoom(ZOOM_OUT)} />
      {onFoldAll && (
        <Button size="sm" onClick={onFoldAll}>
          Fold technical steps
        </Button>
      )}
    </div>
  );
}

/** One step of the zoom buttons and of the plus key. */
export const ZOOM_IN = 1.25;
/** One step of the minus key and the zoom-out button. */
export const ZOOM_OUT = 0.8;
