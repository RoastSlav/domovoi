import { useCallback, useMemo } from "react";
import { useNavigate } from "react-router";
import type { useIncidents } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import { useMediaQuery } from "../../app/useMediaQuery";
import { Button, DataTable, InlineError, Stack } from "../../components";
import { incidentGroupOf, severityMeta } from "../../design/tone";
import { IncidentCardRow } from "./IncidentCardRow";
import { hasTriage, incidentColumns } from "./incidentColumns";
import styles from "./incidents.module.css";

/**
 * From this width the list is a table (its fixed columns need the room); below it, cards
 * with the same facts in the same order.
 */
export const TABLE_QUERY = "(min-width: 1280px)";

/** One group's incidents, as the infinite query reports them. */
export type IncidentsQuery = ReturnType<typeof useIncidents>;

/** What a resolved row looks like: quiet. Nothing is tinted red: the stripe says how bad it is. */
function rowState(incident: IncidentSummary): "quiet" | undefined {
  return incidentGroupOf(incident) === "resolved" ? "quiet" : undefined;
}

/** The stripe follows severity, not status; a constant, so it never makes a row redraw. */
const rowTone = (incident: IncidentSummary) => severityMeta(incident.severity).tone;

/**
 * The loaded incidents of `query`: a table on a wide screen, cards on a narrow one, then
 * Load more with its own progress and its own error. The caller decides what loading, an
 * empty list and a failed first load look like.
 */
export function IncidentList({
  query,
  hostNames,
  caption,
  hideStatus = false,
  showTriage: forceTriage,
}: {
  /** Names the list for assistive technology. */
  caption: string;
  /** Leave out the Status column: the group already says it. */
  hideStatus?: boolean;
  hostNames: Map<string, string>;
  query: IncidentsQuery;
  /**
   * Whether the Triage column is drawn; by default when some row here has a view. The
   * groups of one page share one answer, so their columns line up.
   */
  showTriage?: boolean;
}) {
  const wide = useMediaQuery(TABLE_QUERY);
  const pages = query.data?.pages;
  const items = useMemo(() => pages?.flatMap((page) => page.items) ?? [], [pages]);
  const own = useMemo(() => hasTriage(items), [items]);
  const showTriage = forceTriage ?? own;
  const tableColumns = useMemo(
    () => incidentColumns(hostNames, { hideStatus, showTriage }),
    [hostNames, hideStatus, showTriage],
  );
  const navigate = useNavigate();
  // The whole row opens the incident; the title stays the link for the keyboard.
  const open = useCallback(
    (incident: IncidentSummary) => void navigate(`/incidents/${incident.id}`),
    [navigate],
  );
  const more = () => void query.fetchNextPage();
  return (
    <Stack gap={3}>
      {wide ? (
        <DataTable
          caption={caption}
          columns={tableColumns}
          rows={items}
          rowKey={(incident) => incident.id}
          rowTone={rowTone}
          rowState={rowState}
          layout="fixed"
          onRowClick={open}
        />
      ) : (
        <ul className={styles.cards} aria-label={caption}>
          {items.map((incident) => (
            <IncidentCardRow
              key={incident.id}
              incident={incident}
              host={hostNames.get(incident.host_id)}
              hideStatus={hideStatus}
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
