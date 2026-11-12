import { useState } from "react";
import { Button, Card, Cluster, Markdown, Stack, Tag } from "../../components";
import { Icon } from "../../design/icons";
import styles from "./digest.module.css";

/** How many of a host's lines show before "Show more". */
export const VISIBLE_LINES = 6;

/** One host's part of the digest: its lines, the first few until the person asks for the rest. */
export function HostCard({
  host,
  lines,
  quiet,
}: {
  host: string;
  lines: readonly string[];
  quiet: boolean;
}) {
  const [all, setAll] = useState(false);
  const hidden = Math.max(0, lines.length - VISIBLE_LINES);
  const shown = all ? lines : lines.slice(0, VISIBLE_LINES);
  return (
    <Card>
      <Stack gap={2}>
        <Cluster justify="between" gap={2}>
          <h3 className={styles.cardTitle}>
            <Icon name="server" size={16} />
            {host}
          </h3>
          {quiet && <Tag tone="neutral">Nothing changed</Tag>}
        </Cluster>
        {!quiet && shown.length > 0 && (
          <div className={styles.lines}>
            <Markdown text={shown.map((line) => `- ${line}`).join("\n")} breaks />
          </div>
        )}
        {!quiet && hidden > 0 && (
          <Button size="sm" variant="ghost" aria-expanded={all} onClick={() => setAll(!all)}>
            {all ? "Show fewer" : `Show ${hidden} more`}
          </Button>
        )}
      </Stack>
    </Card>
  );
}
