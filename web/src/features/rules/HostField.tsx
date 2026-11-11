import { useHosts } from "../../api/hooks/hosts";
import { Field, LoadError, Select } from "../../components";

/**
 * The host choice of the add form: "Any host" and every host by name. While the list loads
 * the select says so, and when it cannot load the field gives way to the error with Retry,
 * so a failed request never looks like a host list with nothing in it.
 */
export function HostField({
  value,
  error,
  onChange,
}: {
  error?: string | undefined;
  onChange: (host: string) => void;
  value: string;
}) {
  const hosts = useHosts();
  if (hosts.data === undefined && hosts.error) {
    return <LoadError error={hosts.error} context="load hosts" onRetry={hosts.refetch} />;
  }
  const loading = hosts.data === undefined;
  const names = (hosts.data ?? []).map((host) => host.name);
  if (value !== "" && !names.includes(value) && !loading) {
    names.push(value);
  }
  return (
    <Field label="Host" hint="Leave it on Any host to cover every host." error={error}>
      <Select value={value} disabled={loading} onChange={(event) => onChange(event.target.value)}>
        <option value="">{loading ? "Loading hosts…" : "Any host"}</option>
        {loading && value !== "" && <option value={value}>{value}</option>}
        {names.map((name) => (
          <option key={name} value={name}>
            {name}
          </option>
        ))}
      </Select>
    </Field>
  );
}
