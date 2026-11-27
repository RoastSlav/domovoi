import { useExpectedSignals } from "../../api/hooks/rules";
import type { ExpectedSignal } from "../../api/types";
import {
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Text,
  type Column,
} from "../../components";

const COLUMNS: Column<ExpectedSignal>[] = [
  {
    key: "what",
    label: "What happened",
    truncate: "ellipsis",
    tip: (row) => row.summary,
    cell: (row) => <Text>{row.summary}</Text>,
  },
  {
    key: "check",
    label: "Check",
    width: "12rem",
    cell: (row) => <Text mono>{row.check}</Text>,
  },
  {
    key: "reason",
    label: "Why it is fine",
    truncate: "ellipsis",
    tip: (row) => row.rule_note ?? "The known condition was removed.",
    cell: (row) => <Text muted>{row.rule_note ?? "Known condition removed"}</Text>,
  },
  {
    key: "time",
    label: "When",
    width: "9rem",
    cell: (row) => <RelativeTime at={row.observed_at} />,
  },
];

/**
 * Events a known condition explained on this host. They made no incident and no
 * notification; this list is where a person can still see that they happened.
 */
export function ExpectedTab({ host }: { host: string }) {
  const signals = useExpectedSignals(host);
  return (
    <QueryBoundary
      query={signals}
      context="load the expected events"
      loading={<Skeleton shape="rows" count={4} />}
      isEmpty={(rows) => rows.length === 0}
      empty={
        <EmptyState icon="check" title={`No expected events on ${host}`}>
          <p>
            When a known condition explains a listener, login or failed sudo, it is recorded here
            with the reason. Nothing is raised for it.
          </p>
        </EmptyState>
      }
    >
      {(rows) => (
        <DataTable
          caption={`Expected events on ${host}`}
          layout="fixed"
          columns={COLUMNS}
          rows={rows}
          rowKey={(row) => row.id}
        />
      )}
    </QueryBoundary>
  );
}
