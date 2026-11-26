import { DataTable, RelativeTime, StatusPill, Visible, type Column } from "../../components";
import type { OperationSummary } from "../../api/types";
import { operationStatusMeta } from "../../design/tone";
import styles from "./operations.module.css";

function Hosts({ hosts }: { hosts: string[] }) {
  if (hosts.length === 0) {
    return "–";
  }
  return hosts.length > 2
    ? `${hosts.slice(0, 2).join(", ")} +${hosts.length - 2}`
    : hosts.join(", ");
}

/** Operations shown in the table, with their hosts by display name. */
export interface OperationRow {
  operation: OperationSummary;
  hosts: string[];
}

const COLUMNS: Column<OperationRow>[] = [
  {
    key: "goal",
    label: "Operation",
    truncate: "clamp2",
    tip: (row) => row.operation.goal,
    cell: (row) => <span className={styles.goal}>{row.operation.goal}</span>,
  },
  {
    key: "status",
    label: "Status",
    width: "11rem",
    cell: (row) => <StatusPill size="sm" meta={operationStatusMeta(row.operation.status)} />,
  },
  { key: "hosts", label: "Hosts", width: "10rem", cell: (row) => <Hosts hosts={row.hosts} /> },
  {
    key: "updated",
    label: "Updated",
    width: "8rem",
    cell: (row) => <RelativeTime at={row.operation.updated_at} />,
  },
];

/** The operations as a table; each row opens its page. */
export function OperationsTable({ rows }: { rows: OperationRow[] }) {
  return (
    <Visible category="P0" id="operations.table">
      <DataTable
        caption="Operations"
        columns={COLUMNS}
        rows={rows}
        rowKey={(row) => row.operation.id}
        rowHref={(row) => `/operations/${row.operation.id}`}
      />
    </Visible>
  );
}
