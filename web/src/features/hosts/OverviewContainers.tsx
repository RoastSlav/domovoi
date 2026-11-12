import { useState } from "react";
import { Button, DataTable, Section, StatusPill, Text, type Column } from "../../components";
import { containerStateMeta } from "../../design/tone";
import { Stale, Updated } from "./Freshness";
import { isProblemState, type ContainerRow } from "./containerRows";

/** Besides every container that has a problem, this many others show before "Show all". */
const OTHERS_SHOWN = 5;

const COLUMNS: Column<ContainerRow>[] = [
  {
    key: "name",
    label: "Container",
    width: "14rem",
    truncate: "ellipsis",
    tip: (row) => row.name,
    cell: (row) => (
      <Text mono strong>
        {row.name}
      </Text>
    ),
  },
  {
    key: "state",
    label: "State",
    width: "10rem",
    cell: (row) =>
      row.state === "other" ? (
        <Text muted>{row.word || "unknown"}</Text>
      ) : (
        <>
          <StatusPill size="sm" meta={containerStateMeta(row.state)} />
          <Stale at={row.at} />
        </>
      ),
  },
  {
    key: "restarts",
    label: "Restarts",
    width: "6rem",
    numeric: true,
    cell: (row) => row.restarts,
  },
  {
    key: "image",
    label: "Image",
    truncate: "ellipsis",
    tip: (row) => row.image,
    cell: (row) => <Text mono>{row.image || "–"}</Text>,
  },
  {
    key: "ports",
    label: "Ports",
    width: "16rem",
    truncate: "ellipsis",
    tip: (row) => row.ports,
    cell: (row) => <Text mono>{row.ports === "" ? "–" : row.ports}</Text>,
  },
];

/**
 * The host's containers in one table: the ones that need a person first (unhealthy,
 * restarting, exited), then a few others, with a button for the rest.
 */
export function OverviewContainers({ host, rows }: { host: string; rows: ContainerRow[] }) {
  const [all, setAll] = useState(false);
  if (rows.length === 0) {
    return null;
  }
  const problems = rows.filter((row) => isProblemState(row.state)).length;
  const shown = all ? rows : rows.slice(0, problems + OTHERS_SHOWN);
  const canToggle = rows.length > problems + OTHERS_SHOWN;
  return (
    <Section
      title={`Containers (${rows.length})`}
      description={
        <>
          <Updated facts={rows.map((row) => ({ observed_at: row.at }))} />
          {problems > 0 && ` ${problems} need attention.`}
        </>
      }
      actions={
        canToggle ? (
          <Button size="sm" onClick={() => setAll((now) => !now)}>
            {all ? "Show fewer" : `Show all ${rows.length}`}
          </Button>
        ) : undefined
      }
    >
      <DataTable
        caption={`Containers on ${host}`}
        layout="fixed"
        columns={COLUMNS}
        rows={shown}
        rowKey={(row) => row.name}
        rowTone={(row) =>
          isProblemState(row.state) ? containerStateMeta(row.state).tone : undefined
        }
      />
    </Section>
  );
}
