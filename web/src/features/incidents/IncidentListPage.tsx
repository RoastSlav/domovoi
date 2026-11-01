import { useEffect, useRef } from "react";
import { Link, useSearchParams } from "react-router";
import { useHostNames, useHosts } from "../../api/hooks/hosts";
import { useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { RelativeTime } from "../../components/RelativeTime";
import styles from "./incidents.module.css";

const STATUSES = ["pending", "open", "acked", "muted", "resolved"];
const SEVERITIES = ["critical", "warning", "info"];
type FilterName = keyof IncidentFilters;

function Filter({
  name,
  label,
  options,
  value,
  onChange,
}: {
  name: string;
  label: string;
  options: string[];
  value: string;
  onChange: (value: string) => void;
}) {
  const id = `filter-${name}`;
  return (
    <div className={styles.filter}>
      <label htmlFor={id}>{label}</label>
      <select id={id} value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">Any</option>
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </div>
  );
}

function IncidentRow({ incident, host }: { incident: IncidentSummary; host: string | undefined }) {
  return (
    <li className={styles.row}>
      <div className={styles.rowHead}>
        <Badge value={incident.verdict ?? incident.severity} />
        <Link to={`/incidents/${incident.id}`} className={styles.title}>
          {incident.title}
        </Link>
      </div>
      <p className={styles.meta}>
        {host ?? "unknown host"} · {incident.status} · last fired{" "}
        <RelativeTime at={incident.last_firing_at} />
        {incident.occurrences > 1 && ` · ${incident.occurrences} times`}
      </p>
    </li>
  );
}

/** Loads the next page when the end of the list scrolls into view. */
function useLoadWhenVisible(load: () => void, enabled: boolean) {
  const sentinel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const target = sentinel.current;
    if (!enabled || !target || typeof IntersectionObserver === "undefined") {
      return undefined;
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        load();
      }
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [load, enabled]);
  return sentinel;
}

/** Incidents, newest first, filtered by status, host and severity from the URL. */
export function IncidentListPage() {
  const [params, setParams] = useSearchParams();
  const filters: IncidentFilters = {
    status: params.get("status") ?? undefined,
    host: params.get("host") ?? undefined,
    severity: params.get("severity") ?? undefined,
  };
  const incidents = useIncidents(filters);
  const hosts = useHosts();
  const hostNames = useHostNames();
  const { fetchNextPage, hasNextPage, isFetchingNextPage } = incidents;
  const loadMore = () => void fetchNextPage();
  const sentinel = useLoadWhenVisible(loadMore, hasNextPage && !isFetchingNextPage);
  const items = incidents.data?.pages.flatMap((page) => page.items) ?? [];

  const setFilter = (name: FilterName, value: string) => {
    const next = new URLSearchParams(params);
    if (value) {
      next.set(name, value);
    } else {
      next.delete(name);
    }
    setParams(next);
  };

  return (
    <>
      <h1>Incidents</h1>
      <form className={styles.filters} role="search" aria-label="Filter incidents">
        <Filter
          name="status"
          label="Status"
          options={STATUSES}
          value={filters.status ?? ""}
          onChange={(v) => setFilter("status", v)}
        />
        <Filter
          name="host"
          label="Host"
          options={(hosts.data ?? []).map((h) => h.name)}
          value={filters.host ?? ""}
          onChange={(v) => setFilter("host", v)}
        />
        <Filter
          name="severity"
          label="Severity"
          options={SEVERITIES}
          value={filters.severity ?? ""}
          onChange={(v) => setFilter("severity", v)}
        />
      </form>
      {incidents.isPending && <p>Loading…</p>}
      {incidents.error && <EmptyState title="Could not load incidents" />}
      {incidents.data && items.length === 0 && (
        <EmptyState title="No incidents">
          <p>Nothing matches these filters.</p>
        </EmptyState>
      )}
      {items.length > 0 && (
        <ul className={styles.list}>
          {items.map((incident) => (
            <IncidentRow
              key={incident.id}
              incident={incident}
              host={hostNames.get(incident.host_id)}
            />
          ))}
        </ul>
      )}
      {hasNextPage && (
        <div className={styles.more} ref={sentinel}>
          <Button onClick={loadMore} disabled={isFetchingNextPage}>
            {isFetchingNextPage ? "Loading…" : "Load more"}
          </Button>
        </div>
      )}
    </>
  );
}
