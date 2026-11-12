import type { ReactNode } from "react";
import { columnsFor } from "./profileOverview";
import styles from "./profileTiles.module.css";

/**
 * A row of tiles that fills even rows at every width: all on one row when wide (up to six),
 * three to a row at medium width, two on a phone, and never one tile left alone on its row.
 */
export function TileGrid({ tiles }: { tiles: ReactNode[] }) {
  const [narrow, mid, wide] = [2, 3, 6].map((max) => columnsFor(tiles.length, max));
  const lone = (columns: number | undefined) => tiles.length % (columns ?? 1) === 1 || undefined;
  return (
    <div className={styles.wrap}>
      <div
        className={styles.tiles}
        data-narrow={narrow}
        data-mid={mid}
        data-wide={wide}
        data-lone-narrow={lone(narrow)}
        data-lone-mid={lone(mid)}
        data-lone-wide={lone(wide)}
      >
        {tiles}
      </div>
    </div>
  );
}
