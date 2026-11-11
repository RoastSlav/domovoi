import { useState, type ReactNode } from "react";
import { Link } from "react-router";
import { ENROLL_POLL_MS, useHosts } from "../../api/hooks/hosts";
import type { Invite } from "../../api/types";
import {
  Cluster,
  CopyBlock,
  Countdown,
  InlineMessage,
  Spinner,
  Text,
  useRemaining,
} from "../../components";
import { hostPath } from "./HostRowMenu";
import styles from "./hostsList.module.css";

/** One numbered step of the enrollment, its badge reflecting where the person is. */
export function Step({
  state,
  title,
  children,
}: {
  children: ReactNode;
  state: "pending" | "active" | "done";
  title: string;
}) {
  return (
    <li className={styles.step} data-state={state}>
      <div className={styles.stepBody}>
        <p className={styles.stepTitle}>{title}</p>
        {children}
      </div>
    </li>
  );
}

/**
 * The live end of the enrollment: waits for the agent and says so, then says which host
 * connected. It polls the host list only while it is waiting, and stops once a host that was
 * not in the list when the invite was made connects, or the invite runs out.
 */
export function WaitForAgent({
  invite,
  known,
}: {
  invite: Invite;
  /** The host ids there were when the invite was created; unknown if the list had not loaded. */
  known: ReadonlySet<string> | undefined;
}) {
  const remaining = useRemaining(invite.expires_at);
  const [arrived, setArrived] = useState<string>();
  const hosts = useHosts(arrived === undefined && remaining > 0 ? ENROLL_POLL_MS : false);
  const found = hosts.data?.find(
    (host) => host.connected && (host.id === invite.host_id || (known && !known.has(host.id))),
  );
  if (found && arrived === undefined) {
    setArrived(found.name);
  }

  if (arrived !== undefined) {
    return (
      <InlineMessage tone="success" title={`${arrived} connected`}>
        <p>
          Its agent is reporting. <Link to={hostPath(arrived)}>Open {arrived}</Link>
        </p>
      </InlineMessage>
    );
  }
  if (remaining === 0) {
    return (
      <InlineMessage tone="warning" title="The invite expired">
        <p>No agent used it in time. Create a new invite to try again.</p>
      </InlineMessage>
    );
  }
  return (
    <Cluster gap={2}>
      <Spinner />
      <span role="status">Waiting for the agent to connect…</span>
    </Cluster>
  );
}

/** Steps two and three: the command to run on the host, then the wait for its agent. */
export function InviteSteps({
  invite,
  known,
}: {
  invite: Invite | undefined;
  known: ReadonlySet<string> | undefined;
}) {
  return (
    <>
      <Step title="Copy the command" state={invite ? "active" : "pending"}>
        {invite ? <Command invite={invite} /> : <Text muted>The command appears here.</Text>}
      </Step>
      <Step title="Wait for the agent" state={invite ? "active" : "pending"}>
        {invite ? (
          <WaitForAgent key={invite.token} invite={invite} known={known} />
        ) : (
          <Text muted>This page notices when the agent connects.</Text>
        )}
      </Step>
    </>
  );
}

function Command({ invite }: { invite: Invite }) {
  const remaining = useRemaining(invite.expires_at);
  return (
    <>
      <p>
        Run this on the host. The token works once.{" "}
        {remaining > 0 ? (
          <>
            The invite expires in <Countdown remaining={remaining} until={invite.expires_at} />.
          </>
        ) : (
          "The invite has expired."
        )}
      </p>
      <CopyBlock text={invite.command} label="Enrollment command" />
      <p>The agent pins the hub by this fingerprint. Check it matches what the agent prints:</p>
      <CopyBlock text={invite.fingerprint} label="Hub fingerprint" />
    </>
  );
}
