import { useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import {
  EmptyState,
  InlineMessage,
  LinkButton,
  LoadError,
  Section,
  Skeleton,
} from "../../components";
import { incidentGroupMeta, type IncidentGroup } from "../../design/tone";
import { hasTriage } from "./incidentColumns";
import { IncidentList, type IncidentsQuery } from "./IncidentList";

/** The groups shown open, most urgent first. Resolved follows them, closed. */
const ACTIVE: IncidentGroup[] = ["needs_action", "monitor", "pending", "expected", "handled"];

function hasRows(query: IncidentsQuery): boolean {
  return Boolean(query.data?.pages.some((page) => page.items.length > 0));
}

/**
 * Every incident, as one section per group with the most urgent first; a group with nothing
 * in it is left out, and resolved incidents wait in a closed section. Each group is its own
 * request, filtered on the hub, made in a fixed order.
 */
export function IncidentGroups({
  filters,
  hostNames,
}: {
  filters: IncidentFilters;
  hostNames: Map<string, string>;
}) {
  const queries: Record<IncidentGroup, IncidentsQuery> = {
    needs_action: useIncidents({ ...filters, group: "needs_action" }),
    monitor: useIncidents({ ...filters, group: "monitor" }),
    pending: useIncidents({ ...filters, group: "pending" }),
    expected: useIncidents({ ...filters, group: "expected" }),
    handled: useIncidents({ ...filters, group: "handled" }),
    resolved: useIncidents({ ...filters, group: "resolved" }),
  };
  const all = Object.values(queries);
  // A failed request is never read as an empty group: "nothing needs you" must never be
  // said about incidents the hub could not be asked about. With data on screen the rows
  // stay, since a refetch that failed only means they may be stale.
  const failed = all.find((query) => query.error && !query.data);
  if (failed) {
    return (
      <LoadError
        error={failed.error}
        context="load incidents"
        onRetry={() => all.forEach((query) => void query.refetch())}
      />
    );
  }
  if (all.some((query) => query.isPending)) {
    return (
      <>
        <span className="visually-hidden" role="status">
          Loading…
        </span>
        <Skeleton shape="rows" count={6} />
      </>
    );
  }

  const stale = all.some((query) => query.error && query.data && !query.isFetchNextPageError);
  const active = ACTIVE.filter((group) => hasRows(queries[group]));
  const anyResolved = hasRows(queries.resolved);
  const filtered = Boolean(filters.q ?? filters.severity);
  // One answer for every group, so the tables of the page share their columns.
  const showTriage = all.some((query) =>
    hasTriage(query.data?.pages.flatMap((p) => p.items) ?? []),
  );

  return (
    <>
      {stale && (
        <InlineMessage tone="warning" title="Could not refresh">
          This list may be out of date.
        </InlineMessage>
      )}
      {active.length === 0 &&
        (filtered ? (
          <EmptyState icon="search" title="Nothing open matches">
            <p>No open incident matches the search and severity you chose.</p>
          </EmptyState>
        ) : anyResolved ? (
          <EmptyState icon="check-circle" title="No open incidents">
            <p>Everything your agents watch is healthy.</p>
          </EmptyState>
        ) : (
          <EmptyState
            icon="server"
            title="No incidents yet"
            action={<LinkButton to="/hosts">Go to hosts</LinkButton>}
          >
            <p>
              Domovoi opens an incident when a check on one of your hosts fails. Add a host, or look
              at the checks of one you have.
            </p>
          </EmptyState>
        ))}
      {active.map((group) => (
        <Section
          key={group}
          title={incidentGroupMeta(group).label}
          description={incidentGroupMeta(group).hint}
        >
          <IncidentList
            query={queries[group]}
            hostNames={hostNames}
            caption={incidentGroupMeta(group).label}
            hideStatus={group === "needs_action"}
            showTriage={showTriage}
          />
        </Section>
      ))}
      {anyResolved && (
        <Section
          title="Resolved"
          collapsed
          description="Problems that cleared, the most recently opened first."
        >
          <IncidentList
            query={queries.resolved}
            hostNames={hostNames}
            caption="Resolved"
            showTriage={showTriage}
          />
        </Section>
      )}
    </>
  );
}
