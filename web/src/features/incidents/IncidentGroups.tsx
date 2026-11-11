import { Link } from "react-router";
import { INCIDENT_PAGE, useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import type { IncidentPage, IncidentSummary } from "../../api/types";
import { InlineMessage, LoadError, Loading } from "../../components";
import { IncidentRow } from "./IncidentRow";
import styles from "./incidents.module.css";

/** Resolved rows the closed group shows before the link to all of them. */
const RESOLVED_PREVIEW = 20;

/** One status's incidents, as the infinite query reports them. */
type StatusQuery = ReturnType<typeof useIncidents>;

/** A section of the incident list: its heading, the statuses it holds, and what it says about them. */
interface GroupDef {
  /** The heading's text, counting the group's incidents after it. */
  title: string;
  /** The statuses in this group, in the order their queries run. */
  statuses: string[];
  /** Muted text under the heading, saying what a visit to this group means. */
  hint: string;
  /** The group starts closed and links to the full list of its status. */
  collapsed?: boolean;
  /** What to say when the group is empty; a group without this hides itself. */
  empty?: string;
  /** Which of the group's loaded rows it shows. Being handled takes the open incidents whose
   * verdict says nothing is expected of anyone: dismissed ones, and ones triage itself judged
   * expected. The list has no record of a dismissal, so the verdict is what the grouping has. */
  accept?: (incident: IncidentSummary) => boolean;
}

const GROUPS: GroupDef[] = [
  {
    title: "Needs attention",
    statuses: ["open"],
    hint: "New problems nobody has acknowledged. Expected ones are under Being handled.",
    empty: "Nothing needs attention.",
    accept: (incident) => incident.verdict !== "EXPECTED",
  },
  {
    title: "Being handled",
    statuses: ["acked", "muted", "open"],
    hint:
      "Acknowledged, muted, or expected: dismissed as a known condition, or judged expected by triage. " +
      "A condition that returns after the reopen window is a new incident, and lands under Needs attention.",
    accept: (incident) => incident.status !== "open" || incident.verdict === "EXPECTED",
  },
  {
    title: "Waiting for confirmation",
    statuses: ["pending"],
    hint: "Not confirmed yet. Domovoi alerts if the check keeps failing.",
  },
  {
    title: "Resolved",
    statuses: ["resolved"],
    hint: `The ${RESOLVED_PREVIEW} most recently cleared problems.`,
    collapsed: true,
  },
];

/**
 * A group's loaded incidents, newest firing first, and whether it has more than the hub gave.
 * A source counts as having more only when it already has rows in the group: rows the group
 * does not show say nothing about its size, so the count stays exact for a group whose rows
 * come from a fully loaded status.
 */
function load(
  sources: IncidentPage[][],
  accept: (incident: IncidentSummary) => boolean,
): { items: IncidentSummary[]; more: boolean } {
  const items = sources
    .flat()
    .flatMap((page) => page.items)
    .filter(accept)
    // Fractional seconds vary in length ("…:45Z" and "…:45.5Z"), so string order is not time order.
    .sort((a, b) => Date.parse(b.last_firing_at) - Date.parse(a.last_firing_at));
  return {
    items,
    more: sources.some(
      (pages) =>
        pages.some((page) => page.next_cursor != null) &&
        pages.some((page) => page.items.some(accept)),
    ),
  };
}

/** One group: its heading, hint, rows, and a link to all of them when it is closed. */
function Group({
  def,
  sources,
  pending,
  hostNames,
}: {
  def: GroupDef;
  sources: IncidentPage[][];
  pending: boolean;
  hostNames: Map<string, string>;
}) {
  const { items, more } = load(sources, def.accept ?? (() => true));
  if (items.length === 0 && (pending || !def.empty)) {
    return null;
  }
  const headingId = `group-${def.statuses.join("-")}`;
  const heading = (
    <h2 id={headingId} className={styles.groupHeading}>
      {def.title} ({more ? `${INCIDENT_PAGE}+` : items.length})
    </h2>
  );
  const rows = def.collapsed ? items.slice(0, RESOLVED_PREVIEW) : items;
  const body = (
    <>
      {def.collapsed ? <summary className={styles.groupSummary}>{heading}</summary> : heading}
      <p className={styles.groupHint}>{def.hint}</p>
      {rows.length > 0 && (
        <ul className={styles.list}>
          {rows.map((incident) => (
            <IncidentRow
              key={incident.id}
              incident={incident}
              host={hostNames.get(incident.host_id)}
            />
          ))}
        </ul>
      )}
      {rows.length === 0 && def.empty && <p className={styles.groupEmpty}>{def.empty}</p>}
      {def.collapsed && (
        <Link to="/incidents?status=resolved" className={styles.groupLink}>
          All resolved incidents
        </Link>
      )}
    </>
  );

  return (
    <section className={styles.group} aria-labelledby={headingId}>
      {def.collapsed ? <details>{body}</details> : body}
    </section>
  );
}

/**
 * The incident list as sections by state. The queries are one per status, made in a fixed
 * order, so the hooks do not depend on what each group turns out to hold.
 */
export function IncidentGroups({
  filters,
  hostNames,
}: {
  filters: IncidentFilters;
  hostNames: Map<string, string>;
}) {
  const byStatus: Record<string, StatusQuery> = {
    open: useIncidents({ ...filters, status: "open" }),
    acked: useIncidents({ ...filters, status: "acked" }),
    muted: useIncidents({ ...filters, status: "muted" }),
    pending: useIncidents({ ...filters, status: "pending" }),
    resolved: useIncidents({ ...filters, status: "resolved" }),
  };
  const queries = Object.values(byStatus);
  const loading = queries.some((incidents) => incidents.isPending);
  const loaded = queries.some((incidents) => incidents.data);
  // A failed request is never read as an empty group: "nothing needs attention" must
  // never be said about incidents the hub could not be asked about. With data on screen
  // the rows stay, since a refetch that failed only means they may be stale.
  const unreadable = queries.some((incidents) => incidents.error && !incidents.data);
  const stale = queries.some((incidents) => incidents.error && incidents.data);

  if (unreadable) {
    const failed = queries.find((incidents) => incidents.error && !incidents.data);
    return (
      <LoadError
        error={failed?.error}
        context="load incidents"
        onRetry={() => queries.forEach((incidents) => void incidents.refetch())}
      />
    );
  }

  return (
    <>
      {loading && !loaded && <Loading />}
      {stale && (
        <InlineMessage tone="warning" title="Could not refresh">
          This list may be out of date.
        </InlineMessage>
      )}
      {GROUPS.map((def) => (
        <Group
          key={def.title}
          def={def}
          sources={def.statuses.map((status) => byStatus[status]?.data?.pages ?? [])}
          pending={def.statuses.some((status) => byStatus[status]?.isPending ?? false)}
          hostNames={hostNames}
        />
      ))}
    </>
  );
}
