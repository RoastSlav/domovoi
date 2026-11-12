import { useCallback, useMemo } from "react";
import { Link, useNavigate } from "react-router";
import type { Action } from "../../api/types";
import { DataTable, RelativeTime, StatusPill, Text, type Column } from "../../components";
import { actionStatusMeta } from "../../design/tone";

/** Who decided, as words: the hub names a person as `user:<id>`, and says nothing for the rest. */
export function decidedBy(action: Action): string {
  if (action.approved_by) {
    return action.approved_by.startsWith("user:") ? "A person" : action.approved_by;
  }
  switch (action.status) {
    case "expired":
      return "Nobody, it expired";
    case "rejected":
      return "The agent";
    default:
      return "–";
  }
}

function columns(hosts: Map<string, string>): Column<Action>[] {
  return [
    {
      key: "skill",
      label: "Action",
      cell: (action) => (
        <Link to={`/incidents/${action.incident_id}`}>
          <Text mono strong>
            {action.skill}
          </Text>
        </Link>
      ),
    },
    {
      key: "decision",
      label: "Decision",
      cell: (action) => <StatusPill size="sm" meta={actionStatusMeta(action.status)} />,
    },
    { key: "host", label: "Host", cell: (action) => hosts.get(action.host_id) ?? "Unknown host" },
    { key: "by", label: "Decided by", cell: decidedBy },
    { key: "when", label: "When", cell: (action) => <RelativeTime at={action.updated_at} /> },
  ];
}

/**
 * The actions decided in the last day as one compact table: what, the decision, where, who
 * and when. A row opens the incident the action was for.
 */
export function DecidedList({
  actions,
  hosts,
}: {
  actions: readonly Action[];
  hosts: Map<string, string>;
}) {
  const navigate = useNavigate();
  const tableColumns = useMemo(() => columns(hosts), [hosts]);
  const open = useCallback(
    (action: Action) => void navigate(`/incidents/${action.incident_id}`),
    [navigate],
  );
  return (
    <DataTable
      caption="Recently decided"
      columns={tableColumns}
      rows={actions}
      rowKey={(action) => action.id}
      onRowClick={open}
    />
  );
}
