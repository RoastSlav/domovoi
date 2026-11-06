import { useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useIncidents, type IncidentFilters } from "../../api/hooks/incidents";
import { useSelectedHost } from "../../app/HostContext";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import { IncidentGroups } from "./IncidentGroups";
import { IncidentRow } from "./IncidentRow";
import styles from "./incidents.module.css";

const STATUSES = ["pending", "open", "acked", "muted", "resolved"];
const SEVERITIES = ["critical", "warning", "info"];

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

/**
 * Incidents, newest first, filtered by status and severity from the URL and by the host
 * selected in the sidebar.
 * With no status filter the page groups the incidents by state; with one it lists them.
 */
export function IncidentListPage() {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  const filters: IncidentFilters = {
    status: params.get("status") ?? undefined,
    host,
    severity: params.get("severity") ?? undefined,
  };
  const grouped = !filters.status;
  const incidents = useIncidents(filters, !grouped);
  const hostNames = useHostNames();
  const { fetchNextPage, hasNextPage, isFetchingNextPage } = incidents;
  const loadMore = () => void fetchNextPage();
  const sentinel = useLoadWhenVisible(loadMore, hasNextPage && !isFetchingNextPage);
  const items = incidents.data?.pages.flatMap((page) => page.items) ?? [];

  const setFilter = (name: "status" | "severity", value: string) => {
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
      <PageIntro topic="incidents" docs="concepts/incidents.html">
        Problems Domovoi found on your hosts. Confirmed ones are posted to chat, and triaged when a
        model is set up.
      </PageIntro>
      <form className={styles.filters} role="search" aria-label="Filter incidents">
        <Filter
          name="status"
          label="Status"
          options={STATUSES}
          value={filters.status ?? ""}
          onChange={(v) => setFilter("status", v)}
        />
        <Filter
          name="severity"
          label="Severity"
          options={SEVERITIES}
          value={filters.severity ?? ""}
          onChange={(v) => setFilter("severity", v)}
        />
      </form>
      {grouped ? (
        <IncidentGroups filters={filters} hostNames={hostNames} />
      ) : (
        <>
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
      )}
    </>
  );
}
