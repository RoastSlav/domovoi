import { useHosts } from "../../api/hooks/hosts";

/** Picks global notes ("") or one host's notes, by host name. */
export function HostScope({
  id,
  value,
  onChange,
}: {
  id: string;
  value: string;
  onChange: (host: string) => void;
}) {
  const hosts = useHosts();
  return (
    <select id={id} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">Global</option>
      {(hosts.data ?? []).map((host) => (
        <option key={host.id} value={host.name}>
          {host.name}
        </option>
      ))}
    </select>
  );
}
