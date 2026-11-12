import { useState } from "react";
import type { Fact, Host, HostCheck, LogLine } from "../api/types";
import { Button, Cluster, PageHeader, Stack } from "../components";
import { CheckDialog } from "../features/hosts/CheckDialog";
import { CheckTable } from "../features/hosts/CheckTable";
import { draftOf, type Draft } from "../features/hosts/checkDraft";
import { HostActions } from "../features/hosts/HostActions";
import { HostMeta } from "../features/hosts/HostMeta";
import { OverviewFacts } from "../features/hosts/OverviewFacts";
import { LogViewer, type LogsQuery } from "../features/logs/LogViewer";
import { Demo, Group } from "./Demo";

const MINUTE = 60_000;
const ago = (minutes: number) => new Date(Date.now() - minutes * MINUTE).toISOString();

const HOST: Host = {
  id: "00000000-0000-7000-8000-000000000001",
  name: "nas-01",
  kind: "agent",
  agent_enrolled: true,
  connected: true,
  connected_since: ago(190),
  open_incidents: 2,
  outdated: true,
  version: "0.0.6",
};

const FACTS: Fact[] = [
  { key: "system.uptime", label: "Uptime", value: "41", unit: "days", observed_at: ago(3) },
  { key: "temperature.cpu", label: "CPU temperature", value: "58", unit: "C", observed_at: ago(3) },
  { key: "cpu.usage", label: "CPU usage", value: "34", unit: "%", observed_at: ago(3) },
  { key: "disk.root.used_pct", label: "Disk / used", value: "58", unit: "%", observed_at: ago(12) },
  { key: "disk.root.free", label: "Free on /", value: "112", unit: "GB", observed_at: ago(12) },
  {
    key: "disk.srv.used_pct",
    label: "Disk /srv used",
    value: "91",
    unit: "%",
    observed_at: ago(12),
  },
  {
    key: "container.web.state",
    label: "web state",
    value: "running (healthy)",
    observed_at: ago(3),
  },
  { key: "container.web.image", label: "web image", value: "nginx:1.27", observed_at: ago(3) },
  {
    key: "container.db.state",
    label: "db state",
    value: "exited (1) 3 hours ago",
    observed_at: ago(3),
  },
  { key: "zfs.tank.health", label: "Pool tank", value: "ONLINE", observed_at: ago(12) },
  { key: "memory.available", label: "Available", value: "6.1", unit: "GB", observed_at: ago(3) },
  { key: "net.eth0.rx", label: "Received", value: "3.2", unit: "GB", observed_at: ago(3) },
  { key: "custom.backup.age", label: "Last backup", value: "7", unit: "h", observed_at: ago(30) },
];

// The generated type for thresholds is too narrow for numbers, so the fixtures say it once.
const thresholds = (value: Record<string, number>) => value as unknown as Record<string, never>;

const CHECKS: HostCheck[] = [
  {
    key: "disk_free",
    reported: {
      enabled: true,
      interval_seconds: 300,
      thresholds: thresholds({ warn_percent: 80 }),
    },
    overrides: {
      enabled: null,
      interval_seconds: 120,
      thresholds: thresholds({ warn_percent: 70 }),
    },
  },
  {
    key: "service_nginx",
    reported: { enabled: true, interval_seconds: 30, thresholds: {} },
    overrides: { enabled: null, interval_seconds: null, thresholds: {} },
  },
  {
    key: "smart_health",
    reported: { enabled: true, interval_seconds: 3600, thresholds: {} },
    overrides: { enabled: false, interval_seconds: null, thresholds: {} },
  },
  { key: "ssh_logins", reported: null, overrides: null },
];

const LINES: LogLine[] = [
  [58, "info", "domovoi_agent::session", "Connected to the hub"],
  [47, "debug", "domovoi_agent::checks", "disk_free ran in 41 ms"],
  [41, "warn", "domovoi_agent::checks", "disk_free: / is 83% full"],
  [33, "info", "domovoi_agent::checks", "service_nginx is active"],
  [20, "error", "domovoi_agent::docker", "Cannot reach the Docker socket: permission denied"],
  [19, "warn", "domovoi_agent::docker", "Retrying the Docker socket in 30 s"],
  [8, "info", "domovoi_agent::session", "Heartbeat acknowledged"],
  [2, "debug", "domovoi_agent::checks", "smart_health is switched off by the hub"],
].map(([minutes, level, target, message]) => ({
  at: ago(minutes as number),
  level: level as string,
  target: target as string,
  message: message as string,
}));

const noop = () => undefined;
const logs = (over: Partial<LogsQuery>): LogsQuery => ({
  data: LINES,
  error: null,
  isPending: false,
  refetch: noop,
  ...over,
});

/** The host header, facts, checks editor and log viewer as the Fleet pages draw them. */
export function FleetDemos() {
  const [live, setLive] = useState(true);
  const [editing, setEditing] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const check = CHECKS[0];
  return (
    <Group title="Fleet">
      <Demo name="Host header: HostMeta and HostActions">
        <Stack gap={4}>
          <PageHeader
            title={HOST.name}
            meta={<HostMeta host={HOST} />}
            actions={<HostActions host={HOST} />}
          />
          <PageHeader
            title="pi-garage"
            meta={
              <HostMeta
                host={{
                  ...HOST,
                  name: "pi-garage",
                  connected: false,
                  connected_since: null,
                  last_seen: ago(2000),
                  open_incidents: 0,
                  outdated: false,
                }}
              />
            }
          />
        </Stack>
      </Demo>
      <Demo name="Facts dashboard: OverviewFacts, with and without facts">
        <Stack gap={4}>
          <OverviewFacts name={HOST.name} facts={FACTS} />
          <OverviewFacts name="pi-garage" facts={[]} />
        </Stack>
      </Demo>
      <Demo name="Checks row editor: CheckTable and CheckDialog">
        <Stack gap={3}>
          <CheckTable
            checks={CHECKS}
            connected
            drafts={drafts}
            failing={new Set(["service_nginx"])}
            onEdit={() => setEditing(true)}
          />
          <Cluster>
            <Button onClick={() => setEditing(true)}>Open the edit dialog</Button>
          </Cluster>
          {editing && check && (
            <CheckDialog
              check={check}
              draft={drafts[check.key] ?? draftOf(check)}
              issues={{ interval: "Interval must be at least 1 second." }}
              onApply={(draft) => {
                setDrafts({ ...drafts, [check.key]: draft });
                setEditing(false);
              }}
              onClose={() => setEditing(false)}
            />
          )}
        </Stack>
      </Demo>
      <Demo name="Log viewer: error, warn, info and debug lines">
        <Stack gap={4}>
          <LogViewer
            query={logs({})}
            level="debug"
            label="Logs of nas-01"
            refreshMs={live ? 5000 : null}
            onLiveChange={setLive}
          />
          <LogViewer
            query={logs({ data: undefined, isPending: true })}
            level="info"
            label="Loading"
            refreshMs={null}
          />
          <LogViewer query={logs({ data: [] })} level="error" label="Empty" refreshMs={null} />
          <LogViewer
            query={logs({ data: undefined, error: new TypeError("Failed to fetch") })}
            level="info"
            label="Failed"
            refreshMs={null}
          />
        </Stack>
      </Demo>
    </Group>
  );
}
