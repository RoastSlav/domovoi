import { useMemo } from "react";
import { Link, useSearchParams } from "react-router";
import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import { useOperations } from "../../api/hooks/operations";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  FilterChips,
  InlineMessage,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  type Chip,
} from "../../components";
import { operationStatusMeta } from "../../design/tone";
import { mergeRows } from "./operationRows";
import { OperationsTable } from "./OperationsTable";
import styles from "./operations.module.css";

const STATUSES = ["waiting_approval", "waiting_user", "paused", "running"] as const;
type ChipValue = "all" | (typeof STATUSES)[number];

const CHIPS: Chip<ChipValue>[] = [
  { value: "all", label: "All" },
  ...STATUSES.map((status): Chip<ChipValue> => {
    const meta = operationStatusMeta(status);
    return { value: status, label: meta.label, tone: meta.tone };
  }),
];

/**
 * What Domovoi is investigating and fixing: operations, and the old actions labelled Legacy,
 * newest first. The hub filters by status and host; the page only merges the two sources.
 */
export function OperationsPage() {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  const hostNames = useHostNames();
  const chosen = params.get("status");
  const status: ChipValue = CHIPS.find((chip) => chip.value === chosen)?.value ?? "all";
  const operations = useOperations({
    status: status === "all" ? undefined : status,
    host,
  });
  // Classic actions have no status like the others: only "all" and "waiting for approval" apply.
  const legacyApplies = status === "all" || status === "waiting_approval";
  const actions = useActions(
    { host, status: status === "waiting_approval" ? "proposed" : undefined },
    legacyApplies,
  );

  const pages = operations.data?.pages;
  const operationItems = useMemo(() => pages?.flatMap((page) => page.items) ?? [], [pages]);
  const legacyItems = actions.data?.items;
  const rows = useMemo(
    () =>
      mergeRows({
        operations: operationItems,
        legacy: legacyApplies ? (legacyItems ?? []).filter((action) => action.legacy) : [],
        hostNames,
        moreOperations: operations.hasNextPage,
        moreLegacy: Boolean(actions.data?.next_cursor),
      }),
    [operationItems, legacyItems, legacyApplies, hostNames, operations.hasNextPage, actions.data],
  );

  const choose = (value: ChipValue) => {
    const next = new URLSearchParams(params);
    if (value === "all") {
      next.delete("status");
    } else {
      next.set("status", value);
    }
    setParams(next, { replace: true });
  };
  const legacyLoading = legacyApplies && actions.isPending;

  return (
    <>
      <PageHeader
        title="Operations"
        docs="concepts/operations.html"
        intro="What Domovoi is investigating and fixing, and what waits for you."
      />
      <Stack gap={4}>
        <FilterChips label="Status" options={CHIPS} value={status} onChange={choose} />
        {legacyApplies && actions.isError && (
          <InlineMessage
            tone="warning"
            title="Could not load classic actions"
            action={
              <Button size="sm" onClick={() => void actions.refetch()}>
                Retry
              </Button>
            }
          />
        )}
        <QueryBoundary
          query={operations}
          context="load operations"
          loading={<Skeleton shape="rows" count={6} />}
        >
          {() =>
            rows.length === 0 && legacyLoading ? (
              <Skeleton shape="rows" count={6} />
            ) : rows.length === 0 ? (
              <EmptyState
                icon="play"
                title={status === "all" ? "No operations yet" : "No operations in this state"}
                action={<Link to="/approvals">Go to Approvals</Link>}
              >
                <p>
                  With orchestration on, each new incident is investigated as an operation. Classic
                  approvals stay on the Approvals page.
                </p>
              </EmptyState>
            ) : (
              <Stack gap={3}>
                <OperationsTable rows={rows} />
                {operations.hasNextPage && (
                  <div className={styles.more}>
                    <Button
                      onClick={() => void operations.fetchNextPage()}
                      loading={operations.isFetchingNextPage}
                    >
                      Load more
                    </Button>
                  </div>
                )}
              </Stack>
            )
          }
        </QueryBoundary>
      </Stack>
    </>
  );
}
