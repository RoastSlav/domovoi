import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import { useOverview } from "../../api/hooks/overview";
import type { Overview } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  EmptyState,
  FilterChips,
  PageHeader,
  QueryBoundary,
  Select,
  Skeleton,
  TextInput,
  useDebounced,
  type Chip,
} from "../../components";
import {
  INCIDENT_GROUP_ORDER,
  incidentGroupMeta,
  severityMeta,
  type IncidentGroup,
} from "../../design/tone";
import { IncidentGroups } from "./IncidentGroups";
import { IncidentList } from "./IncidentList";
import styles from "./incidents.module.css";

const SEVERITIES = ["critical", "warning", "info"];

/** How long typing in the search box rests before the list is asked again. */
export const SEARCH_DELAY_MS = 300;

type Counts = Overview["incident_groups"];
type ChipValue = "all" | IncidentGroup;

/** What each group's emptiness means, as a sentence. */
const EMPTY: Record<IncidentGroup, string> = {
  needs_action: "Nothing needs action right now.",
  monitor: "Nothing to keep an eye on right now.",
  pending: "Nothing is waiting to be confirmed.",
  expected: "No open incident is marked as expected.",
  handled: "Nothing is acknowledged or muted.",
  resolved: "Nothing has resolved yet.",
};

function chips(counts: Counts | undefined): Chip<ChipValue>[] {
  const groups = INCIDENT_GROUP_ORDER.map((group): Chip<ChipValue> => {
    const meta = incidentGroupMeta(group);
    const value = group === "resolved" ? 0 : (counts?.[group] ?? 0);
    return {
      value: group,
      label: meta.label,
      tone: meta.tone,
      ...(value > 0 ? { count: { value, label: "incidents" } } : {}),
    };
  });
  return [{ value: "all", label: "All" }, ...groups];
}

/** One group on its own: the hub filters it, and its emptiness is said in words. */
function OneGroup({
  group,
  filters,
  hostNames,
}: {
  filters: IncidentFilters;
  group: IncidentGroup;
  hostNames: Map<string, string>;
}) {
  const incidents = useIncidents({ ...filters, group });
  const label = incidentGroupMeta(group).label;
  return (
    <QueryBoundary
      query={incidents}
      context="load incidents"
      loading={<Skeleton shape="rows" count={6} />}
      isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
      empty={
        <EmptyState title={EMPTY[group]} action={<Link to="/incidents">Show all incidents</Link>}>
          {(filters.q ?? filters.severity) && <p>The search and severity you chose still apply.</p>}
        </EmptyState>
      }
    >
      {() => (
        <IncidentList
          query={incidents}
          hostNames={hostNames}
          caption={label}
          hideStatus={group === "needs_action"}
        />
      )}
    </QueryBoundary>
  );
}

/**
 * Incidents, filtered by group, text and severity from the URL (so a view can be shared and
 * survives a reload) and by the host selected for the whole app. With no group chosen the
 * page shows every group, the most urgent first.
 */
export function IncidentListPage() {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  const hostNames = useHostNames();
  const overview = useOverview(host);
  const chosen = params.get("group");
  const group = INCIDENT_GROUP_ORDER.find((known) => known === chosen);
  const q = params.get("q") ?? "";
  const [text, setText] = useState(q);
  const settled = useDebounced(text.trim(), SEARCH_DELAY_MS);

  const setParam = (name: string, value: string, replace = false) => {
    const next = new URLSearchParams(params);
    if (value) {
      next.set(name, value);
    } else {
      next.delete(name);
    }
    setParams(next, { replace });
  };
  useEffect(() => {
    if (settled !== q) {
      setParam("q", settled, true);
    }
    // Only a settled search writes the URL; the URL changing must not write it back.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settled]);

  const filters: IncidentFilters = {
    host,
    q: q || undefined,
    severity: params.get("severity") ?? undefined,
  };

  return (
    <>
      <PageHeader
        title="Incidents"
        docs="concepts/incidents.html"
        intro="Problems Domovoi found on your hosts."
      />
      <form
        className={styles.filters}
        role="search"
        aria-label="Filter incidents"
        onSubmit={(event) => event.preventDefault()}
      >
        <FilterChips
          label="Group"
          value={group ?? "all"}
          options={chips(overview.data?.incident_groups)}
          onChange={(value) => setParam("group", value === "all" ? "" : value)}
        />
        <div className={styles.filterRow}>
          <TextInput
            type="search"
            aria-label="Search"
            placeholder="Search title, check or target"
            value={text}
            onChange={(event) => setText(event.target.value)}
          />
          <Select
            aria-label="Severity"
            value={filters.severity ?? ""}
            onChange={(event) => setParam("severity", event.target.value)}
          >
            <option value="">Any severity</option>
            {SEVERITIES.map((severity) => (
              <option key={severity} value={severity}>
                {severityMeta(severity).label}
              </option>
            ))}
          </Select>
        </div>
      </form>
      {group ? (
        <OneGroup group={group} filters={filters} hostNames={hostNames} />
      ) : (
        <IncidentGroups filters={filters} hostNames={hostNames} />
      )}
    </>
  );
}
