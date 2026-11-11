import { Link } from "react-router";
import type { useIncidents } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import { useMediaQuery } from "../../app/useMediaQuery";
import {
  Button,
  Card,
  Cluster,
  DataTable,
  InlineError,
  RelativeTime,
  Stack,
  StatusPill,
  Text,
  type Column,
} from "../../components";
import { incidentGroupOf, incidentStateMeta, severityMeta } from "../../design/tone";
import { triageSaid } from "./badge";
import { IncidentRowMenu } from "./IncidentRowMenu";
import styles from "./incidents.module.css";

/** From this width the list is a table; below it, cards with the same facts in the same order. */
export const TABLE_QUERY = "(min-width: 900px)";

/** One group's incidents, as the infinite query reports them. */
export type IncidentsQuery = ReturnType<typeof useIncidents>;

function checkText(incident: IncidentSummary): string {
  return incident.target ? `${incident.check} ${incident.target}` : incident.check;
}

function rowState(incident: IncidentSummary): "urgent" | "quiet" | undefined {
  const group = incidentGroupOf(incident);
  return group === "needs_action" ? "urgent" : group === "resolved" ? "quiet" : undefined;
}

function columns(hostNames: Map<string, string>): Column<IncidentSummary>[] {
  return [
    {
      key: "state",
      label: "Status",
      cell: (incident) => <StatusPill meta={incidentStateMeta(incident)} />,
    },
    {
      key: "title",
      label: "Incident",
      cell: (incident) => (
        <Link to={`/incidents/${incident.id}`}>
          <Text strong>{incident.title}</Text>
        </Link>
      ),
    },
    {
      key: "host",
      label: "Host",
      cell: (incident) => hostNames.get(incident.host_id) ?? "Unknown host",
    },
    {
      key: "check",
      label: "Check",
      cell: (incident) => (
        <Text mono size="sm">
          {checkText(incident)}
        </Text>
      ),
    },
    {
      key: "severity",
      label: "Severity",
      cell: (incident) => <StatusPill size="sm" meta={severityMeta(incident.severity)} />,
    },
    {
      key: "fired",
      label: "Last fired",
      cell: (incident) => <RelativeTime at={incident.last_firing_at} />,
    },
    {
      key: "times",
      label: "Times",
      numeric: true,
      hint: "How many times the check has fired for this incident.",
      cell: (incident) => incident.occurrences,
    },
    { key: "triage", label: "Triage", cell: (incident) => triageSaid(incident) },
    {
      key: "actions",
      label: "Actions",
      hideLabel: true,
      cell: (incident) => <IncidentRowMenu incident={incident} />,
    },
  ];
}

/** One incident as a card, for narrow screens: the table's facts in the table's order. */
function IncidentCardRow({
  incident,
  host,
}: {
  incident: IncidentSummary;
  host: string | undefined;
}) {
  const state = incidentStateMeta(incident);
  const note = triageSaid(incident);
  return (
    <li>
      <Card tone={state.tone}>
        <Stack gap={2}>
          <Cluster justify="between" align="start" wrap={false}>
            <Stack gap={1}>
              <Cluster>
                <StatusPill meta={state} />
              </Cluster>
              <Link to={`/incidents/${incident.id}`}>
                <Text strong>{incident.title}</Text>
              </Link>
            </Stack>
            <IncidentRowMenu incident={incident} />
          </Cluster>
          <Cluster>
            <Text size="sm">{host ?? "Unknown host"}</Text>
            <Text mono size="sm">
              {checkText(incident)}
            </Text>
            <StatusPill size="sm" meta={severityMeta(incident.severity)} />
          </Cluster>
          <Cluster>
            <Text muted size="sm">
              Last fired <RelativeTime at={incident.last_firing_at} />
            </Text>
            {incident.occurrences > 1 && (
              <Text muted size="sm">
                {incident.occurrences} times
              </Text>
            )}
            {note && (
              <Text muted size="sm">
                {note}
              </Text>
            )}
          </Cluster>
        </Stack>
      </Card>
    </li>
  );
}

/**
 * The loaded incidents of `query`: a table on a wide screen, cards on a narrow one, then
 * Load more with its own progress and its own error. The caller decides what loading, an
 * empty list and a failed first load look like.
 */
export function IncidentList({
  query,
  hostNames,
  caption,
}: {
  /** Names the list for assistive technology. */
  caption: string;
  hostNames: Map<string, string>;
  query: IncidentsQuery;
}) {
  const wide = useMediaQuery(TABLE_QUERY);
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];
  const more = () => void query.fetchNextPage();
  return (
    <Stack gap={3}>
      {wide ? (
        <DataTable
          caption={caption}
          columns={columns(hostNames)}
          rows={items}
          rowKey={(incident) => incident.id}
          rowTone={(incident) => incidentStateMeta(incident).tone}
          rowState={rowState}
        />
      ) : (
        <ul className={styles.cards} aria-label={caption}>
          {items.map((incident) => (
            <IncidentCardRow
              key={incident.id}
              incident={incident}
              host={hostNames.get(incident.host_id)}
            />
          ))}
        </ul>
      )}
      {query.isFetchNextPageError ? (
        <InlineError
          error={query.error}
          context="load more incidents"
          action={
            <Button size="sm" onClick={more}>
              Retry
            </Button>
          }
        />
      ) : (
        query.hasNextPage && (
          <div className={styles.more}>
            <Button onClick={more} loading={query.isFetchingNextPage}>
              Load more
            </Button>
          </div>
        )
      )}
    </Stack>
  );
}
