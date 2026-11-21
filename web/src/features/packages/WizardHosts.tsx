import { useHosts } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import {
  Button,
  Checkbox,
  Cluster,
  QueryBoundary,
  Skeleton,
  StatusPill,
  Text,
  visibilityAttrs,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../design/tone";
import styles from "./wizard.module.css";
import { MAX_HOSTS, canContinue, type WizardState } from "./wizardState";

/** What the person should know about going on with this host, or nothing. */
function noteFor(connection: string, minutes: string): string | undefined {
  switch (connection) {
    case "offline":
    case "never_connected":
      return `Not connected now. If you go on, the install waits up to ${minutes} for it.`;
    case "incompatible":
    case "expected_incompatible":
      return "Needs an agent upgrade first. It will be skipped.";
    case "hub_only":
      return "No agent on this host.";
    default:
      return undefined;
  }
}

/** One host's choice, with its connection and what that means for the install. */
function HostChoice({
  host,
  checked,
  minutes,
  onChange,
}: {
  checked: boolean;
  host: Host;
  minutes: string;
  onChange: (checked: boolean) => void;
}) {
  const connection = hostConnectionOf(host);
  const note = noteFor(connection, minutes);
  return (
    <li className={styles.hostRow} {...visibilityAttrs("P1", "install.hosts.row")}>
      <Checkbox
        label={host.name}
        checked={checked}
        disabledReason={connection === "hub_only" ? "No agent on this host." : undefined}
        onChange={onChange}
      />
      <div className={styles.hostState} {...visibilityAttrs("P0", "install.hosts.state")}>
        <StatusPill size="sm" meta={hostConnectionMeta(connection)} />
        {note && connection !== "hub_only" && (
          <Text muted size="sm">
            {note}
          </Text>
        )}
      </div>
    </li>
  );
}

/**
 * Step two: which hosts. A host with no agent is shown disabled; one that is offline or needs
 * an upgrade says what going on means. Not for deciding whether an install can run: the plan
 * on the next step does that.
 */
export function WizardHosts({
  state,
  set,
  minutes,
  blocked,
  onBack,
  onNext,
}: {
  blocked: string | undefined;
  minutes: string;
  onBack: () => void;
  onNext: () => void;
  set: (patch: Partial<WizardState>) => void;
  state: WizardState;
}) {
  const hosts = useHosts();
  const gate = canContinue(state);
  const toggle = (id: string, on: boolean) =>
    set({ hostIds: on ? [...state.hostIds, id] : state.hostIds.filter((item) => item !== id) });
  return (
    <>
      <Text block>
        {`${state.kind === "uninstall" ? "Remove" : "Install"} ${state.name} ${state.version} on:`}
      </Text>
      <QueryBoundary query={hosts} context="load hosts" loading={<Skeleton shape="rows" />}>
        {(list) => (
          <ul className={styles.hostList} aria-label="Hosts">
            {list.map((host) => (
              <HostChoice
                key={host.id}
                host={host}
                minutes={minutes}
                checked={state.hostIds.includes(host.id)}
                onChange={(on) => toggle(host.id, on)}
              />
            ))}
          </ul>
        )}
      </QueryBoundary>
      <div className={styles.nav}>
        <Cluster gap={3} justify="between">
          <Button onClick={onBack}>Back</Button>
          <Text muted>{`${state.hostIds.length} of up to ${MAX_HOSTS}`}</Text>
          <Button
            variant="primary"
            disabledReason={(gate.ok ? undefined : gate.reason) ?? blocked}
            onClick={onNext}
          >
            Next
          </Button>
        </Cluster>
      </div>
    </>
  );
}
