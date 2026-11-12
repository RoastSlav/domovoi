import { KeyValueList, Tag, Text, Tooltip } from "../../components";
import { shown } from "./factValue";
import { Stale, Updated } from "./Freshness";
import { diskTone, type DiskRow, type StatusLine } from "./diskRows";
import { OverviewPanel } from "./OverviewPanel";
import styles from "./overview.module.css";

/** What a full disk says in words, so the bar colour is not the only signal. */
const FULLNESS = { warning: "Filling up", danger: "Almost full" } as const;

function Disk({ row }: { row: DiskRow }) {
  const tone = diskTone(row);
  const amount = [row.pct === undefined ? "" : `${shown(row.pct)} %`, row.free ?? ""]
    .filter(Boolean)
    .join(" · ");
  return (
    <li className={styles.disk}>
      <Tooltip content={row.mount}>
        <span className={styles.mount}>
          <Text mono strong>
            {row.mount}
          </Text>
        </span>
      </Tooltip>
      {row.pct === undefined ? (
        <span className={styles.noBar} />
      ) : (
        <progress
          className={styles.bar}
          data-tone={tone}
          aria-label={`${row.mount} used`}
          value={Math.min(row.pct, 100)}
          max={100}
        />
      )}
      <span className={styles.amount}>
        {tone === "warning" || tone === "danger" ? <Tag tone={tone}>{FULLNESS[tone]}</Tag> : null}{" "}
        <Text>{amount}</Text>
        <Stale at={row.at} />
      </span>
    </li>
  );
}

/**
 * One compact row per mount or pool, fullest first: its path, a thin bar and the percent
 * used with what is free, warning from 85 % and danger from 95 %. Pool, RAID and SMART state
 * are small status lines under the rows.
 */
export function OverviewDisks({ rows, lines }: { lines: StatusLine[]; rows: DiskRow[] }) {
  if (rows.length === 0 && lines.length === 0) {
    return null;
  }
  const seen = [...rows, ...lines].map((entry) => ({ observed_at: entry.at }));
  return (
    <OverviewPanel title="Disks" updated={<Updated facts={seen} />}>
      {rows.length > 0 && (
        <ul className={styles.disks} aria-label="Disk usage">
          {rows.map((row) => (
            <Disk key={row.mount} row={row} />
          ))}
        </ul>
      )}
      {lines.length > 0 && (
        <KeyValueList
          items={lines.map((line) => ({
            label: line.label,
            value: (
              <>
                {line.tone ? <Tag tone={line.tone}>{line.value}</Tag> : line.value}
                <Stale at={line.at} />
              </>
            ),
          }))}
        />
      )}
    </OverviewPanel>
  );
}
