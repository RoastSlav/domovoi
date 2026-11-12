import type { ReactNode } from "react";
import { useSettings } from "../../../api/hooks/system";
import { QueryBoundary, Skeleton, Stack } from "../../../components";
import styles from "../settings.module.css";
import { Advisories } from "./Advisories";
import { HeartbeatStatus } from "./HeartbeatStatus";
import { Usage } from "./Usage";

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack gap={3}>
      <h3 className={styles.subhead}>{title}</h3>
      {children}
    </Stack>
  );
}

/** Model usage, the heartbeat and the open advisories: what the hub is doing, not what it is set to. */
export function StatusSection() {
  const settings = useSettings();
  return (
    <QueryBoundary query={settings} context="load settings" loading={<Skeleton shape="rows" />}>
      {(data) => (
        <Stack gap={6}>
          <Group title="Model usage">
            <Usage settings={data} />
          </Group>
          <Group title="Heartbeat">
            <HeartbeatStatus heartbeat={data.heartbeat} />
          </Group>
          <Group title="Advisories">
            <Advisories />
          </Group>
        </Stack>
      )}
    </QueryBoundary>
  );
}
