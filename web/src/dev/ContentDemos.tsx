import { useState } from "react";
import {
  Button,
  Card,
  Cluster,
  DataTable,
  EmptyState,
  FilterChips,
  Grid,
  KeyValueList,
  Loading,
  PageHeader,
  Progress,
  QueryBoundary,
  Section,
  Skeleton,
  Spinner,
  Stack,
  StatusPill,
  Tabs,
  Tag,
} from "../components";
import { hostConnectionMeta, type Tone } from "../design/tone";
import { Demo, Group } from "./Demo";

/** Loading, empty and error states. */
export function StateDemos() {
  const never = { data: undefined, error: null, isPending: true, refetch: () => undefined };
  return (
    <Group title="Loading, empty, error">
      <Demo name="Loading, Spinner, Progress">
        <Stack gap={3}>
          <Loading />
          <Cluster>
            <Spinner /> <span>Saving…</span>
          </Cluster>
          <Progress label="Triage is reading the logs…" />
        </Stack>
      </Demo>
      <Demo name="Skeleton: line, block, rows">
        <Stack gap={3}>
          <Skeleton shape="line" count={3} />
          <Skeleton shape="block" />
          <Skeleton shape="rows" count={3} />
        </Stack>
      </Demo>
      <Demo name="EmptyState">
        <EmptyState icon="alert" title="No incidents" action={<Button>Add a host</Button>}>
          <p>Everything your agents watch is healthy.</p>
        </EmptyState>
      </Demo>
      <Demo name="QueryBoundary: loading, error, empty, content">
        <Stack gap={2}>
          <QueryBoundary query={never} context="load hosts" loading={<Skeleton shape="rows" />}>
            {() => null}
          </QueryBoundary>
          <QueryBoundary
            query={{ ...never, isPending: false, error: new TypeError("Failed to fetch") }}
            context="load hosts"
          >
            {() => null}
          </QueryBoundary>
          <QueryBoundary
            query={{ ...never, isPending: false, data: [] as string[] }}
            context="load hosts"
            isEmpty={(hosts) => hosts.length === 0}
            empty={<EmptyState title="No hosts yet" />}
          >
            {() => null}
          </QueryBoundary>
          <QueryBoundary
            query={{ ...never, isPending: false, data: ["nas-01"] }}
            context="load hosts"
          >
            {(hosts) => <p>Loaded: {hosts.join(", ")}</p>}
          </QueryBoundary>
        </Stack>
      </Demo>
    </Group>
  );
}

interface HostRow {
  connection: string;
  incidents: number;
  name: string;
  tone?: Tone;
}

const HOSTS: HostRow[] = [
  { name: "nas-01", connection: "connected", incidents: 0 },
  { name: "pi-garage", connection: "offline", incidents: 3, tone: "danger" },
  { name: "hub", connection: "hub_only", incidents: 1, tone: "warning" },
];

/** Layout and data: page header, sections, cards, tables, lists, chips and tabs. */
export function LayoutDemos() {
  const [status, setStatus] = useState("open");
  const [kinds, setKinds] = useState<string[]>(["agent"]);
  return (
    <Group title="Layout and data">
      <Demo name="PageHeader, Section, Card, Grid, Stack, Cluster">
        <PageHeader
          title="nas-01"
          meta={
            <>
              <StatusPill meta={hostConnectionMeta("connected")} />
              <Tag>container agent</Tag>
            </>
          }
          intro="One or two sentences saying what the page is for."
          docs="concepts/hub-and-agent.html"
          topic="hosts"
          actions={<Button variant="primary">Main action</Button>}
        />
        <Section
          title="Section"
          description="What this part of the page holds."
          actions={<Button size="sm">Action</Button>}
        >
          <Grid min="sm">
            <Card>A plain card</Card>
            <Card tone="danger">A card with a danger stripe</Card>
            <Card tone="success" to="/dev/design" label="A linked card">
              A linked card
            </Card>
          </Grid>
        </Section>
      </Demo>
      <Demo name="DataTable (narrow the window to see it stack)">
        <Stack gap={3}>
          <DataTable
            caption="Hosts"
            rows={HOSTS}
            rowKey={(host) => host.name}
            rowTone={(host) => host.tone}
            rowHref={() => "/dev/design"}
            columns={[
              { key: "name", label: "Host", cell: (host) => host.name },
              {
                key: "agent",
                label: "Agent",
                hint: "Whether the host's agent is connected to the hub right now.",
                cell: (host) => <StatusPill meta={hostConnectionMeta(host.connection)} />,
              },
              {
                key: "open",
                label: "Open incidents",
                numeric: true,
                cell: (host) => host.incidents,
              },
            ]}
          />
          <DataTable
            caption="Loading hosts"
            rows={[]}
            rowKey={() => ""}
            loading
            columns={[{ key: "name", label: "Host", cell: () => null }]}
          />
          <DataTable
            caption="No hosts"
            rows={[]}
            rowKey={() => ""}
            empty={<EmptyState title="No hosts yet" />}
            columns={[{ key: "name", label: "Host", cell: () => null }]}
          />
        </Stack>
      </Demo>
      <Demo name="KeyValueList">
        <KeyValueList
          items={[
            { label: "Version", value: "0.0.8" },
            {
              label: "Manifest hash",
              value: <code>a1b2c3d4e5f6</code>,
              hint: "Changes when the skill's files change.",
            },
          ]}
        />
      </Demo>
      <Demo name="FilterChips: single and multiple">
        <Stack gap={2}>
          <FilterChips
            label="Status"
            value={status}
            onChange={setStatus}
            options={[
              { value: "open", label: "Open", tone: "danger", count: { value: 3, label: "open" } },
              { value: "acked", label: "Acknowledged", count: { value: 1, label: "acknowledged" } },
              { value: "resolved", label: "Resolved" },
            ]}
          />
          <FilterChips
            multiple
            label="Host kind"
            value={kinds}
            onChange={setKinds}
            options={[
              { value: "agent", label: "Agent" },
              { value: "container_agent", label: "Container agent" },
              { value: "external", label: "External" },
            ]}
          />
        </Stack>
      </Demo>
      <Demo name="Tabs (arrow keys, Home and End)">
        <Tabs
          label="Host details"
          tabs={[
            { label: "Overview", content: <p>Overview panel</p> },
            {
              label: "Checks",
              content: <p>Checks panel</p>,
              count: { value: 2, label: "failing", tone: "danger" },
            },
            { label: "Logs", content: <p>Logs panel</p> },
          ]}
        />
      </Demo>
    </Group>
  );
}
