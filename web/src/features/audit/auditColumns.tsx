import type { AuditRow } from "../../api/types";
import { IconButton, RelativeTime, Tag, Text, type Column } from "../../components";
import { auditKindHint } from "../../design/auditText";

/** The entry table's columns; `firstBroken` marks the entry where the chain broke. */
export function auditColumns(
  firstBroken: number | undefined,
  open: (row: AuditRow) => void,
): Column<AuditRow>[] {
  return [
    {
      key: "seq",
      label: "No.",
      numeric: true,
      cell: (row) => (
        <>
          #{row.seq}
          {row.seq === firstBroken && (
            <>
              {" "}
              <Tag tone="danger">First broken entry</Tag>
            </>
          )}
        </>
      ),
    },
    { key: "at", label: "When", cell: (row) => <RelativeTime at={row.at} /> },
    {
      key: "actor",
      label: "Who",
      truncate: "ellipsis",
      tip: (row) => row.actor,
      cell: (row) => <Text mono>{row.actor}</Text>,
    },
    {
      key: "kind",
      label: "What",
      truncate: "ellipsis",
      tip: (row) => auditKindHint(row.kind) ?? row.kind,
      cell: (row) => <Text mono>{row.kind}</Text>,
    },
    {
      key: "subject",
      label: "About",
      truncate: "ellipsis",
      tip: (row) => row.subject,
      cell: (row) => <Text mono>{row.subject}</Text>,
    },
    {
      key: "open",
      label: "Open",
      hideLabel: true,
      cell: (row) => (
        <IconButton
          icon="chevron-right"
          label={`Open entry ${row.seq}`}
          variant="ghost"
          size="sm"
          onClick={() => open(row)}
        />
      ),
    },
  ];
}
