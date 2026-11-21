import { useSearchParams } from "react-router";
import { useHosts } from "../../api/hooks/hosts";
import { useShadowDecisions, type ShadowFilters } from "../../api/hooks/policy";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  FilterChips,
  QueryBoundary,
  Skeleton,
  Stack,
  Visible,
  type Chip,
} from "../../components";
import { DecisionList } from "./DecisionList";

type Show = "all" | "differing" | "unevaluable";

const CHIPS: Chip<Show>[] = [
  { value: "all", label: "All decisions" },
  { value: "differing", label: "Where the two differ" },
  { value: "unevaluable", label: "Could not evaluate" },
];

/**
 * The recorded decisions, filtered by what differs or could not be evaluated, and by the
 * selected host.
 * @param recording Whether the hub records decisions at all.
 */
export function DecisionsTab({ recording }: { recording: boolean }) {
  const [params, setParams] = useSearchParams();
  const [selected] = useSelectedHost();
  const hosts = useHosts();
  const asked = params.get("show");
  const show: Show = CHIPS.find((chip) => chip.value === asked)?.value ?? "all";
  const hostId =
    selected === undefined ? undefined : hosts.data?.find((h) => h.name === selected)?.id;
  const waitingForHost = selected !== undefined && hostId === undefined && hosts.isPending;
  const filters: ShadowFilters = {
    host: hostId,
    differing: show === "differing" ? true : undefined,
    status: show === "unevaluable" ? "unevaluable" : undefined,
  };
  const decisions = useShadowDecisions(filters, !waitingForHost);
  const choose = (value: Show) => {
    const next = new URLSearchParams(params);
    if (value === "all") next.delete("show");
    else next.set("show", value);
    setParams(next, { replace: true });
  };
  return (
    <Stack gap={3}>
      <Visible category="P1" id="policy.decision-filters">
        <FilterChips label="Show" options={CHIPS} value={show} onChange={choose} />
      </Visible>
      <QueryBoundary
        query={decisions}
        context="load the decisions"
        loading={<Skeleton shape="rows" count={5} />}
      >
        {(data) => {
          const rows = data.pages.flatMap((page) => page.items);
          if (rows.length === 0) {
            return recording ? (
              <EmptyState icon="eye" title="No decisions">
                {show === "all"
                  ? "No decisions yet. One is recorded for each action an operation proposes."
                  : "No recorded decision matches this filter."}
              </EmptyState>
            ) : (
              <EmptyState icon="eye" title="Decisions are not being recorded">
                Set shadow to true under [policy] in hub.toml and restart the hub to start.
              </EmptyState>
            );
          }
          return (
            <Stack gap={3}>
              <DecisionList rows={rows} />
              {decisions.hasNextPage && (
                <div>
                  <Button
                    onClick={() => void decisions.fetchNextPage()}
                    loading={decisions.isFetchingNextPage}
                  >
                    Load more
                  </Button>
                </div>
              )}
            </Stack>
          );
        }}
      </QueryBoundary>
    </Stack>
  );
}
