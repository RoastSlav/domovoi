import type { ReactNode } from "react";
import { Card, Stack, Text } from "../../components";
import styles from "./overview.module.css";

/** A card with a heading, when it was last updated, and its content. */
export function OverviewPanel({
  title,
  updated,
  children,
}: {
  children: ReactNode;
  title: string;
  updated: ReactNode;
}) {
  return (
    <Card label={title}>
      <Stack gap={3}>
        <div className={styles.panelHead}>
          <Text strong>{title}</Text>
          {updated}
        </div>
        {children}
      </Stack>
    </Card>
  );
}
