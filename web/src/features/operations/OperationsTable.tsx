import { DataTable, RelativeTime, StatusPill, Tag, type Column } from "../../components";
import { actionStatusMeta, operationStatusMeta } from "../../design/tone";
import type { OperationRow } from "./operationRows";
import styles from "./operations.module.css";

const LEGACY_HINT = "From before operations. It finishes the classic way and is not converted.";

function Hosts({ hosts }: { hosts: string[] }) {
  if (hosts.length === 0) {
    return "–";
  }
  return hosts.length > 2
    ? `${hosts.slice(0, 2).join(", ")} +${hosts.length - 2}`
    : hosts.join(", ");
}

const COLUMNS: Column<OperationRow>[] = [
  {
    key: "goal",
    label: "Operation",
    truncate: "clamp2",
    tip: (row) => row.goal,
    cell: (row) => (
      <span className={styles.goal}>
        {row.goal}
        {row.kind === "legacy" && <Tag hint={LEGACY_HINT}>Legacy</Tag>}
      </span>
    ),
  },
  {
    key: "status",
    label: "Status",
    width: "11rem",
    cell: (row) => (
      <StatusPill
        size="sm"
        meta={
          row.kind === "legacy" ? actionStatusMeta(row.status) : operationStatusMeta(row.status)
        }
      />
    ),
  },
  { key: "hosts", label: "Hosts", width: "10rem", cell: (row) => <Hosts hosts={row.hosts} /> },
  {
    key: "updated",
    label: "Updated",
    width: "8rem",
    cell: (row) => <RelativeTime at={row.updatedAt} />,
  },
];

/** The merged operations and legacy actions as a table; each row opens its page. */
export function OperationsTable({ rows }: { rows: OperationRow[] }) {
  return (
    <DataTable
      caption="Operations"
      columns={COLUMNS}
      rows={rows}
      rowKey={(row) => `${row.kind}:${row.id}`}
      rowHref={(row) => row.href}
    />
  );
}
