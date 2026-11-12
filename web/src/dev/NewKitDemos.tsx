import { useCallback, useMemo, useState } from "react";
import {
  Button,
  Checkbox,
  Cluster,
  CommandPalette,
  DataTable,
  Grid,
  Kbd,
  Menu,
  RelativeTime,
  StatTile,
  Stack,
  StatusPill,
  SubmitBar,
  Switch,
  Tag,
  Text,
  type Column,
  type PaletteGroup,
} from "../components";
import { hostConnectionMeta } from "../design/tone";
import { Demo, Group } from "./Demo";

interface HostRow {
  name: string;
  open: number;
  seen: string;
}

const ROWS: HostRow[] = [
  { name: "nas-01", open: 0, seen: new Date(Date.now() - 120_000).toISOString() },
  { name: "pi-garage", open: 3, seen: new Date(Date.now() - 7_200_000).toISOString() },
];

const COLUMNS: readonly Column<HostRow>[] = [
  { key: "name", label: "Host", cell: (row) => row.name },
  { key: "seen", label: "Last seen", cell: (row) => <RelativeTime at={row.seen} /> },
  { key: "open", label: "Open incidents", numeric: true, cell: (row) => row.open },
  {
    key: "menu",
    label: "Actions",
    hideLabel: true,
    cell: (row) => (
      <Menu
        label={`Actions for ${row.name}`}
        items={[{ label: "Edit", onSelect: () => undefined }]}
      />
    ),
  },
];

/** The newer kit parts: stat tiles, the command palette, badges on toggles, clickable rows. */
export function NewKitDemos() {
  const [palette, setPalette] = useState(false);
  const [picked, setPicked] = useState("Nothing chosen yet");
  const [clicked, setClicked] = useState("No row clicked yet");
  const [on, setOn] = useState(true);
  const [box, setBox] = useState(true);
  const onRowClick = useCallback((row: HostRow) => setClicked(`Clicked ${row.name}`), []);
  const groups = useMemo<PaletteGroup[]>(
    () => [
      {
        title: "Pages",
        items: [
          {
            id: "overview",
            label: "Overview",
            icon: "digest",
            onSelect: () => setPicked("Overview"),
          },
          {
            id: "incidents",
            label: "Incidents",
            icon: "alert",
            keywords: ["problems"],
            onSelect: () => setPicked("Incidents"),
          },
        ],
      },
      {
        title: "Hosts",
        items: [
          {
            id: "nas-01",
            label: "nas-01",
            icon: "server",
            hint: "Connected",
            onSelect: () => setPicked("nas-01"),
          },
        ],
      },
      {
        title: "Actions",
        items: [
          {
            id: "theme",
            label: "Toggle theme",
            icon: "moon",
            keywords: ["dark", "light"],
            onSelect: () => setPicked("Toggle theme"),
          },
        ],
      },
    ],
    [],
  );
  return (
    <Group title="Dashboards, search and rows">
      <Demo name="StatTile: plain, with a tone dot and hint, as a link card">
        <Grid min="sm">
          <StatTile label="Open incidents" value={4} />
          <StatTile
            label="Needs attention"
            value={2}
            tone="danger"
            hint="2 hosts have an action needed"
          />
          <StatTile label="Hosts online" value="3 of 4" tone="success" hint="One host is offline" />
          <StatTile label="Waiting for approval" value={1} to="/dev/design" hint="Open the list" />
        </Grid>
      </Demo>
      <Demo name="CommandPalette: opens with the button; type, arrows, Enter and Escape work">
        <Stack gap={2}>
          <Cluster>
            <Button icon="search" onClick={() => setPalette(true)}>
              Search…
            </Button>
            <Kbd>Ctrl</Kbd>
            <Kbd>K</Kbd>
            <Text muted>{picked}</Text>
          </Cluster>
          <CommandPalette open={palette} onClose={() => setPalette(false)} groups={groups} />
        </Stack>
      </Demo>
      <Demo name="Switch and Checkbox with badges beside the label, and a hint under it">
        <Stack gap={3}>
          <Switch
            label="Enable the check"
            hint="Runs every 60 s."
            checked={on}
            onChange={setOn}
            badges={<Tag hint="Set in hub.toml">hub.toml</Tag>}
          />
          <Checkbox
            label="Include logins from the internet"
            checked={box}
            onChange={setBox}
            badges={<Tag>default</Tag>}
          />
        </Stack>
      </Demo>
      <Demo name="DataTable: the whole row is the click target (onRowClick)">
        <Stack gap={2}>
          <DataTable
            caption="Hosts with a clickable row"
            columns={COLUMNS}
            rows={ROWS}
            rowKey={(row) => row.name}
            onRowClick={onRowClick}
            rowTone={(row) => (row.open > 0 ? "danger" : undefined)}
          />
          <Text muted>{clicked}</Text>
          <StatusPill meta={hostConnectionMeta("connected")} />
        </Stack>
      </Demo>
      <Demo name="Menu with shortcut hints, and the floating SubmitBar (always shown here)">
        <Stack gap={3}>
          <Menu
            items={[
              { label: "Go to incidents", shortcut: "G I", onSelect: () => undefined },
              { label: "Go to hosts", shortcut: "G H", onSelect: () => undefined },
            ]}
          />
          <SubmitBar changes={2} alwaysShow onDiscard={() => undefined} onSave={() => undefined}>
            <Tag tone="warning">Restarts the hub</Tag>
          </SubmitBar>
        </Stack>
      </Demo>
    </Group>
  );
}
