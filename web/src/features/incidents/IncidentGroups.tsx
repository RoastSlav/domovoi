import { Link } from "react-router";
import { useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import type { IncidentPage, IncidentSummary } from "../../api/types";
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
}

const GROUPS: GroupDef[] = [
  {
    title: "Needs attention",
    statuses: ["open"],
    hint: "New problems nobody has acknowledged.",
    empty: "Nothing needs attention.",
  },
  {
    title: "Being handled",
    statuses: ["acked", "muted"],
    hint: "Acknowledged or muted; they close when the problem clears.",
  },
  {
    title: "Waiting for confirmation",
    statuses: ["pending"],
    hint: "Seen once. Domovoi alerts if the check fails again.",
  },
  {
    title: "Resolved",
    statuses: ["resolved"],
    hint: "Cleared problems, newest first.",
    collapsed: true,
  },
];

/** A group's loaded incidents, newest firing first, and whether the hub has more than it gave. */
function load(pages: IncidentPage[]): { items: IncidentSummary[]; more: boolean } {
  return {
    items: pages
      .flatMap((page) => page.items)
      .sort((a, b) => b.last_firing_at.localeCompare(a.last_firing_at)),
    more: pages.some((page) => page.next_cursor != null),
  };
}

/** One group: its heading, hint, rows, and a link to all of them when it is closed. */
function Group({
  def,
  pages,
  pending,
  hostNames,
}: {
  def: GroupDef;
  pages: IncidentPage[];
  pending: boolean;
  hostNames: Map<string, string>;
}) {
  const { items, more } = load(pages);
  if (items.length === 0 && (pending || !def.empty)) {
    return null;
  }
  const headingId = `group-${def.statuses.join("-")}`;
  const heading = (
    <h2 id={headingId} className={styles.groupHeading}>
      {def.title} ({more ? "50+" : items.length})
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
  const loading = Object.values(byStatus).some((incidents) => incidents.isPending);

  return (
    <>
      {loading && <p>Loading…</p>}
      {GROUPS.map((def) => (
        <Group
          key={def.title}
          def={def}
          pages={def.statuses.flatMap((status) => byStatus[status]?.data?.pages ?? [])}
          pending={def.statuses.some((status) => byStatus[status]?.isPending ?? false)}
          hostNames={hostNames}
        />
      ))}
    </>
  );
}
