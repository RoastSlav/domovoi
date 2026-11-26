import { useMemo, useState } from "react";
import { useAuditRows } from "../../api/hooks/audit";
import type { AuditRow } from "../../api/types";
import {
  Button,
  DataTable,
  EmptyState,
  InlineMessage,
  QueryBoundary,
  Skeleton,
} from "../../components";
import styles from "./audit.module.css";
import { auditColumns } from "./auditColumns";
import { EntryDialog } from "./EntryDialog";

/** The audit entries, load more, and the entry dialog. */
export function AuditTable({ firstBroken }: { firstBroken: number | undefined }) {
  const rows = useAuditRows();
  const [open, setOpen] = useState<AuditRow | undefined>(undefined);
  const columns = useMemo(() => auditColumns(firstBroken, setOpen), [firstBroken]);
  const all = useMemo(() => rows.data?.pages.flatMap((page) => page.items) ?? [], [rows.data]);
  return (
    <>
      <QueryBoundary
        query={{ ...rows, data: rows.data ? all : undefined }}
        context="load audit entries"
        loading={<Skeleton shape="rows" count={5} />}
        isEmpty={(items) => items.length === 0}
        empty={
          <EmptyState icon="info" title="No entries yet">
            New entries appear here as Domovoi records them.
          </EmptyState>
        }
      >
        {(items) => (
          <DataTable
            caption="Audit entries"
            columns={columns}
            rows={items}
            rowKey={(row) => String(row.seq)}
            rowState={(row) => (row.seq === firstBroken ? "urgent" : undefined)}
          />
        )}
      </QueryBoundary>
      {rows.isFetchNextPageError && (
        <InlineMessage
          tone="danger"
          title="Could not load more entries"
          action={
            <Button size="sm" onClick={() => void rows.fetchNextPage()}>
              Retry
            </Button>
          }
        />
      )}
      {rows.hasNextPage && !rows.isFetchNextPageError && (
        <div className={styles.more}>
          <Button loading={rows.isFetchingNextPage} onClick={() => void rows.fetchNextPage()}>
            Load more
          </Button>
        </div>
      )}
      {open && <EntryDialog row={open} onClose={() => setOpen(undefined)} />}
    </>
  );
}
