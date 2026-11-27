import type { CeilingEntry } from "../../api/types";
import { DataTable, Tag, Text, Tooltip, type Column } from "../../components";
import styles from "./capabilityCenter.module.css";
import { boundsText, formatHold } from "./limitsText";

const SHOWN_NAMES = 6;

/** The targets of one entry: a chip for "any", or a chip per name with the rest in a tooltip. */
function Targets({ targets }: { targets: CeilingEntry["targets"] }) {
  if (targets.kind === "any") {
    return <Tag>Any target</Tag>;
  }
  const shown = targets.names.slice(0, SHOWN_NAMES);
  const rest = targets.names.slice(SHOWN_NAMES);
  return (
    <span className={styles.tags}>
      {shown.map((name) => (
        <Tag key={name}>{name}</Tag>
      ))}
      {rest.length > 0 && <Tag hint={rest.join(", ")}>{`+${rest.length} more`}</Tag>}
    </span>
  );
}

/**
 * Whether the host's helper would serve this capability. The ceiling is the agent's limit; a
 * green row there can still end in a helper refusal, so the helper's answer is its own column.
 */
export function HelperCell({ offer }: { offer: CeilingEntry["helper"] | undefined }) {
  if (!offer || offer.state === "not_applicable") {
    return <Text muted>Not used</Text>;
  }
  if (offer.state === "offered") {
    return <Tag tone="success">Offered</Tag>;
  }
  return (
    <span>
      <Tag tone="warning">Not offered</Tag>
      <Text muted size="sm" block>
        {offer.text}
      </Text>
    </span>
  );
}

const COLUMNS: Column<CeilingEntry>[] = [
  {
    key: "contract",
    label: "Capability",
    cell: (entry) => (
      <Text mono strong>
        {entry.contract}
      </Text>
    ),
  },
  { key: "targets", label: "Targets", cell: (entry) => <Targets targets={entry.targets} /> },
  {
    key: "helper",
    label: "Helper",
    cell: (entry) => <HelperCell offer={entry.helper} />,
  },
  { key: "hour", label: "Per hour", numeric: true, cell: (entry) => entry.max_per_hour },
  {
    key: "active",
    label: "Active at once",
    numeric: true,
    cell: (entry) =>
      entry.max_active === null || entry.max_active === undefined ? (
        <Tooltip content="This entry sets no limit; the host's other limits still apply.">
          <span tabIndex={0}>–</span>
        </Tooltip>
      ) : (
        entry.max_active
      ),
  },
  { key: "hold", label: "Longest hold", cell: (entry) => formatHold(entry.max_ttl_secs) },
  {
    key: "bounds",
    label: "Argument bounds",
    cell: (entry) => (
      <Text mono size="sm">
        <span className={styles.bounds}>{boundsText(entry.args)}</span>
      </Text>
    ),
  },
];

/** The unattended limits one host reported, one row per capability. Display only. */
export function LimitsTable({ host, entries }: { entries: readonly CeilingEntry[]; host: string }) {
  return (
    <DataTable
      caption={`Unattended limits reported by ${host}`}
      columns={COLUMNS}
      rows={entries}
      rowKey={(entry) => entry.contract}
    />
  );
}
