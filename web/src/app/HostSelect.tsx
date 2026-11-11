import { useHosts } from "../api/hooks/hosts";
import { Button, OptionPicker, StatusPill, type PickerOption } from "../components";
import { Icon } from "../design/icons";
import { describeError } from "../design/messages";
import { hostConnectionMeta, hostConnectionOf } from "../design/tone";
import { useSelectedHost } from "./HostContext";
import styles from "./Layout.module.css";

/** The value of the "All hosts" option. */
const ALL = "";

/**
 * The app-wide host choice: every page that filters by host follows it. Each host shows
 * whether its agent is connected (a maintenance tag goes beside it when hosts have one);
 * with many hosts, typing filters the list. A failed load says so and offers Retry, so it
 * never reads as "no hosts".
 */
export function HostSelect() {
  const hosts = useHosts();
  const [selected, select] = useSelectedHost();

  if (hosts.error && !hosts.data) {
    return (
      <span className={styles.hostError} role="alert">
        <Icon name="alert-circle" />
        {describeError(hosts.error, "load hosts").title}
        <Button size="sm" icon="refresh" onClick={() => void hosts.refetch()}>
          Retry
        </Button>
      </span>
    );
  }

  const list = hosts.data ?? [];
  const options: PickerOption[] = [
    { value: ALL, label: "All hosts" },
    // A selection the hub does not list (a stale link) still shows, so it can be seen and cleared.
    ...(selected !== undefined && !list.some((host) => host.name === selected)
      ? [{ value: selected, label: selected }]
      : []),
    ...list.map((host) => ({
      value: host.name,
      label: host.name,
      meta: <StatusPill size="sm" meta={hostConnectionMeta(hostConnectionOf(host))} />,
    })),
  ];
  return (
    <OptionPicker
      label="Host"
      value={selected ?? ALL}
      options={options}
      onChange={(value) => select(value || undefined)}
    />
  );
}
