import type { Host } from "../../api/types";
import { Cluster, FilterChips, TextInput, type Chip } from "../../components";
import { countsOf, SEARCH_FROM, type HostFilter } from "./hostFilter";

/**
 * The chips that narrow the hosts list (all, problems, online, offline, each with its count)
 * and, once there are more than a handful of hosts, a box that searches names.
 */
export function HostsFilters({
  hosts,
  filter,
  query,
  onFilter,
  onQuery,
}: {
  filter: HostFilter;
  hosts: readonly Host[];
  onFilter: (filter: HostFilter) => void;
  onQuery: (query: string) => void;
  query: string;
}) {
  const counts = countsOf(hosts);
  const chip = (value: HostFilter, label: string, noun: string): Chip<HostFilter> => ({
    value,
    label,
    count: { value: counts[value], label: noun },
    ...(value === "problems" && counts.problems > 0 ? { tone: "danger" as const } : {}),
  });
  return (
    <Cluster gap={3} justify="between">
      <FilterChips
        label="Show hosts"
        value={filter}
        onChange={onFilter}
        options={[
          chip("all", "All", "hosts listed"),
          chip("problems", "Problems", "hosts with problems"),
          chip("online", "Online", "hosts connected"),
          chip("offline", "Offline", "hosts not connected"),
        ]}
      />
      {hosts.length > SEARCH_FROM && (
        <TextInput
          type="search"
          aria-label="Search hosts"
          placeholder="Search by name"
          value={query}
          onChange={(event) => onQuery(event.target.value)}
        />
      )}
    </Cluster>
  );
}
