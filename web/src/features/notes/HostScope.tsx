import { useHosts } from "../../api/hooks/hosts";
import { Field, LoadError, Select } from "../../components";

/**
 * Picks global notes ("") or one host's notes, by host name. When the hosts cannot be
 * loaded it says so with Retry rather than offering a list with only "Global" in it.
 */
export function HostScope({
  label,
  value,
  onChange,
}: {
  label: string;
  onChange: (host: string) => void;
  value: string;
}) {
  const hosts = useHosts();
  if (hosts.error && !hosts.data) {
    return <LoadError error={hosts.error} context="load hosts" onRetry={hosts.refetch} />;
  }
  return (
    <Field label={label}>
      <Select value={value} onChange={(event) => onChange(event.target.value)}>
        <option value="">Global</option>
        {(hosts.data ?? []).map((host) => (
          <option key={host.id} value={host.name}>
            {host.name}
          </option>
        ))}
      </Select>
    </Field>
  );
}
