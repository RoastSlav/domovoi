import type { HostCheck } from "../../api/types";
import { Button, DataTable, StatusPill, Tag, Text, type Column } from "../../components";
import { checkStateMeta } from "../../design/tone";
import { thresholdWords } from "./thresholdText";
import { effectiveOf, sameDraft, draftOf, type Draft, type Effective } from "./checkDraft";

/** What the table needs to draw one check. */
interface Row {
  check: HostCheck;
  effective: Effective;
  failing: boolean;
  staged: boolean;
}

function State({ row, connected }: { connected: boolean; row: Row }) {
  if (row.effective.enabled === false) {
    return <StatusPill size="sm" meta={checkStateMeta("disabled")} />;
  }
  if (row.failing) {
    return <StatusPill size="sm" meta={checkStateMeta("failing")} />;
  }
  if (!connected && row.effective.enabled === undefined) {
    return <Text muted>Unknown</Text>;
  }
  return <StatusPill size="sm" meta={checkStateMeta("ok")} />;
}

function columns(connected: boolean, onEdit: (key: string) => void): Column<Row>[] {
  return [
    {
      key: "check",
      label: "Check",
      width: "20rem",
      truncate: "ellipsis",
      tip: (row) => row.check.key,
      cell: (row) => (
        <Text mono strong>
          {row.check.key}
        </Text>
      ),
    },
    {
      key: "state",
      label: "State",
      width: "8rem",
      hint: "Failing means the check has an incident that is not resolved yet.",
      cell: (row) => <State row={row} connected={connected} />,
    },
    {
      key: "interval",
      label: "Runs every",
      width: "8rem",
      numeric: true,
      cell: (row) =>
        row.effective.interval === undefined ? (
          <Text muted>unknown</Text>
        ) : (
          `${row.effective.interval} s`
        ),
    },
    {
      key: "thresholds",
      label: "Thresholds",
      truncate: "clamp2",
      tip: (row) => thresholdWords(row.effective.thresholds),
      cell: (row) =>
        Object.keys(row.effective.thresholds).length === 0 ? (
          <Text muted>none</Text>
        ) : (
          <Text size="sm">{thresholdWords(row.effective.thresholds)}</Text>
        ),
    },
    {
      key: "source",
      label: "Source",
      width: "13rem",
      hint: "Agent default is what agent.toml sets. Hub override is a value set here that wins over it.",
      cell: (row) => (
        <>
          <Tag {...(row.effective.source === "hub" ? { tone: "info" as const } : {})}>
            {row.effective.source === "hub" ? "Hub override" : "Agent default"}
          </Tag>
          {row.staged && <Tag tone="warning">Unsaved</Tag>}
        </>
      ),
    },
    {
      key: "actions",
      label: "Actions",
      hideLabel: true,
      width: "6rem",
      cell: (row) => (
        <Button
          size="sm"
          variant="ghost"
          icon="edit"
          aria-label={`Edit ${row.check.key}`}
          onClick={() => onEdit(row.check.key)}
        >
          Edit
        </Button>
      ),
    },
  ];
}

/**
 * A host's checks, one row each: its state, how often it runs, its thresholds and whether
 * those come from the agent or from a hub override. `drafts` are changes not saved yet; the
 * row shows them as they will be.
 */
export function CheckTable({
  checks,
  connected,
  drafts,
  failing,
  onEdit,
}: {
  checks: readonly HostCheck[];
  connected: boolean;
  drafts: Record<string, Draft>;
  failing: ReadonlySet<string>;
  onEdit: (key: string) => void;
}) {
  const rows: Row[] = checks.map((check) => {
    const staged = drafts[check.key];
    return {
      check,
      failing: failing.has(check.key),
      staged: staged !== undefined && !sameDraft(staged, draftOf(check)),
      effective: effectiveOf(check, staged ?? draftOf(check)),
    };
  });
  return (
    <DataTable
      caption="Checks"
      layout="fixed"
      columns={columns(connected, onEdit)}
      rows={rows}
      rowKey={(row) => row.check.key}
      rowTone={(row) => (row.failing ? "danger" : undefined)}
    />
  );
}
