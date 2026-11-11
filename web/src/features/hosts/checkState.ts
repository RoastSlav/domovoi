import { useIncidents } from "../../api/hooks/incidents";
import { useHostChecks } from "../../api/hooks/settings";

/**
 * The checks of host `name` that have an incident which is not resolved yet, by key, and
 * how many there are. Reads the same incident list and check list the overview and the
 * Checks tab read, so asking again costs no request. The count is 0 until both have loaded.
 */
export function useFailingChecks(name: string): { count: number; keys: ReadonlySet<string> } {
  const incidents = useIncidents({ host: name });
  const checks = useHostChecks(name);
  const keys = new Set<string>();
  for (const page of incidents.data?.pages ?? []) {
    for (const incident of page.items) {
      if (incident.status !== "resolved") {
        keys.add(incident.check);
      }
    }
  }
  const known = new Set(checks.data?.checks.map((check) => check.key));
  return { keys, count: [...keys].filter((key) => known.has(key)).length };
}
