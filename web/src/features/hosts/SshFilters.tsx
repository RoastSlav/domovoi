import { Cluster, FilterChips, type Chip } from "../../components";
import { filterLogins, type LoginEntry, type LoginFilter } from "./sshLogins";

function count(entries: readonly LoginEntry[], filter: LoginFilter, label: string) {
  return { value: filterLogins(entries, filter).length, label };
}

/**
 * The ways to narrow the login history: by how the attempt ended (offered once a failed
 * attempt is in the history) and by where it came from. Each chip counts what it would show,
 * given the other filter.
 */
export function SshFilters({
  entries,
  filter,
  onChange,
}: {
  entries: readonly LoginEntry[];
  filter: LoginFilter;
  onChange: (filter: LoginFilter) => void;
}) {
  const results: Chip<LoginFilter["result"]>[] = [
    { value: "all", label: "All results" },
    {
      value: "accepted",
      label: "Accepted",
      count: count(entries, { ...filter, result: "accepted" }, "accepted logins"),
    },
    {
      value: "failed",
      label: "Failed",
      tone: "warning",
      count: count(entries, { ...filter, result: "failed" }, "failed logins"),
    },
  ];
  const networks: Chip<LoginFilter["network"]>[] = [
    { value: "all", label: "Anywhere" },
    { value: "lan", label: "Local network" },
    { value: "wan", label: "Internet", tone: "warning" },
  ];
  return (
    <Cluster gap={4}>
      {entries.some((entry) => entry.result === "failed") && (
        <FilterChips
          label="Result"
          options={results}
          value={filter.result}
          onChange={(result) => onChange({ ...filter, result })}
        />
      )}
      <FilterChips
        label="Source"
        options={networks}
        value={filter.network}
        onChange={(network) => onChange({ ...filter, network })}
      />
    </Cluster>
  );
}
