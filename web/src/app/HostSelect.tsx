import { useHosts } from "../api/hooks/hosts";
import { Button, OptionPicker, Tooltip, type PickerOption } from "../components";
import { Icon } from "../design/icons";
import { describeError } from "../design/messages";
import { hostConnectionMeta, hostConnectionOf } from "../design/tone";
import { useSelectedHost } from "./HostContext";
import styles from "./topbar.module.css";

/** The value of the "All hosts" option. */
const ALL = "";

/** A host's connection as a coloured dot, with its word kept for assistive technology. */
function ConnectionDot({ connection }: { connection: string }) {
  const meta = hostConnectionMeta(connection);
  return (
    <Tooltip content={meta.hint}>
      <span className={styles.hostDot} data-tone={meta.tone}>
        <Icon name="dot" />
        <span className="visually-hidden">{meta.label}</span>
      </span>
    </Tooltip>
  );
}

/** What an option says about its host, on the right: a dot and the connection in muted words. */
function ConnectionText({ connection }: { connection: string }) {
  const meta = hostConnectionMeta(connection);
  return (
    <span className={styles.optionStatus} data-tone={meta.tone}>
      <span className={styles.hostDot}>
        <Icon name="dot" />
      </span>
      {meta.label}
    </span>
  );
}

/**
 * The app-wide host choice: every page that filters by host follows it, and a page about one
 * host selects that host. The button shows the choice with its connection as a dot (under
 * "All hosts", how many hosts there are); each option shows its host's connection in muted
 * words, and with many hosts, typing filters the list. A failed load says so and offers
 * Retry, so it never reads as "no hosts".
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
    {
      value: ALL,
      label: "All hosts",
      meta: list.length > 0 ? <span aria-hidden="true">{list.length}</span> : undefined,
    },
    // A selection the hub does not list (a stale link) still shows, so it can be seen and cleared.
    ...(selected !== undefined && !list.some((host) => host.name === selected)
      ? [{ value: selected, label: selected }]
      : []),
    ...list.map((host) => ({
      value: host.name,
      label: host.name,
      meta: <ConnectionText connection={hostConnectionOf(host)} />,
    })),
  ];
  const chosen = selected === undefined ? undefined : list.find((host) => host.name === selected);
  const valueMeta = chosen ? (
    <ConnectionDot connection={hostConnectionOf(chosen)} />
  ) : selected === undefined && list.length > 0 ? (
    <span className={styles.hostCount}>{list.length}</span>
  ) : undefined;
  return (
    <OptionPicker
      label="Host"
      icon="server"
      valueMeta={valueMeta}
      value={selected ?? ALL}
      options={options}
      onChange={(value) => select(value || undefined)}
    />
  );
}
