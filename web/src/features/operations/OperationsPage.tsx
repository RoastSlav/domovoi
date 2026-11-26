import { useMemo } from "react";
import { Link, useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useOperations } from "../../api/hooks/operations";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  FilterChips,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  Visible,
  VisibilityPage,
  type Chip,
} from "../../components";
import { operationStatusMeta } from "../../design/tone";
import { OperationsTable, type OperationRow } from "./OperationsTable";
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
 * What Domovoi is investigating and fixing: operations, newest first. The hub filters by
 * status and host.
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

  const pages = operations.data?.pages;
  const operationItems = useMemo(() => pages?.flatMap((page) => page.items) ?? [], [pages]);
  const rows = useMemo(
    (): OperationRow[] =>
      operationItems.map((operation) => ({
        operation,
        hosts: operation.hosts.map((id) => hostNames.get(id) ?? id),
      })),
    [operationItems, hostNames],
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

  return (
    <VisibilityPage page="operations">
      <PageHeader
        title="Operations"
        docs="concepts/operations.html"
        intro="What Domovoi is investigating and fixing, and what waits for you."
      />
      <Stack gap={4}>
        <Visible category="P1" id="operations.filters">
          <FilterChips label="Status" options={CHIPS} value={status} onChange={choose} />
        </Visible>
        <Visible category="P0" id="operations.state">
          <QueryBoundary
            query={operations}
            context="load operations"
            loading={<Skeleton shape="rows" count={6} />}
          >
            {() =>
              rows.length === 0 ? (
                <EmptyState
                  icon="play"
                  title={status === "all" ? "No operations yet" : "No operations in this state"}
                  action={<Link to="/approvals">Go to Approvals</Link>}
                >
                  <p>With orchestration on, each new incident is investigated as an operation.</p>
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
        </Visible>
      </Stack>
    </VisibilityPage>
  );
}
