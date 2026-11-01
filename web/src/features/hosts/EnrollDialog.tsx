import { useState, type FormEvent } from "react";
import { ENROLL_POLL_MS, useCreateInvite, useHosts } from "../../api/hooks/hosts";
import { ApiFailure, type Invite } from "../../api/types";
import { Button } from "../../components/Button";
import { CopyBlock } from "../../components/CopyBlock";
import { Countdown, useRemaining } from "../../components/Countdown";
import { Dialog } from "../../components/Dialog";
import styles from "./hosts.module.css";

function Waiting({ invite, name }: { invite: Invite; name: string }) {
  const hosts = useHosts(ENROLL_POLL_MS);
  const remaining = useRemaining(invite.expires_at);
  const host = hosts.data?.find((h) => h.id === invite.host_id);
  if (host?.connected) {
    return (
      <p className={styles.connected} role="status">
        Connected. {name} is enrolled{host.version ? ` and runs agent ${host.version}` : ""}.
      </p>
    );
  }
  if (remaining === 0) {
    return (
      <p className={styles.expired} role="status">
        This invite expired before an agent used it. Create a new one.
      </p>
    );
  }
  return (
    <p role="status">
      Waiting for the agent… The invite expires in{" "}
      <Countdown remaining={remaining} until={invite.expires_at} />.
    </p>
  );
}

function failureText(error: unknown): string {
  if (error instanceof ApiFailure && error.status === 400) {
    return "Use a host name of lower-case letters, digits and dashes.";
  }
  return "Could not create the invite. Try again.";
}

/**
 * Adding a host: its name, then a one-time invite with the command to run on it and the
 * hub's fingerprint, then waiting until its agent connects.
 */
export function EnrollDialog({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [invite, setInvite] = useState<Invite>();
  const create = useCreateInvite();

  const request = () =>
    create.mutate(name.trim(), {
      onSuccess: (created) => setInvite(created),
    });

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (name.trim()) {
      request();
    }
  };

  return (
    <Dialog
      title={invite ? `Enroll ${name.trim()}` : "Add a host"}
      onClose={onClose}
      wide
      actions={
        <>
          {invite && (
            <Button onClick={request} disabled={create.isPending}>
              New invite
            </Button>
          )}
          <Button variant={invite ? "primary" : "secondary"} onClick={onClose}>
            {invite ? "Done" : "Cancel"}
          </Button>
        </>
      }
    >
      {!invite && (
        <form className={styles.form} onSubmit={onSubmit}>
          <label htmlFor="enroll-name">Host name</label>
          <input
            id="enroll-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoComplete="off"
            spellCheck={false}
            aria-describedby="enroll-name-hint"
          />
          <p id="enroll-name-hint" className={styles.hint}>
            For example <code>nas</code> or <code>pi-garage</code>.
          </p>
          {create.error && (
            <p className={styles.error} role="alert">
              {failureText(create.error)}
            </p>
          )}
          <div>
            <Button type="submit" variant="primary" disabled={!name.trim() || create.isPending}>
              Create invite
            </Button>
          </div>
        </form>
      )}
      {invite && (
        <div className={styles.form}>
          <p>Run this on the host. The token works once.</p>
          <CopyBlock text={invite.command} label="Enrollment command" />
          <p>The agent pins the hub by this fingerprint. Check it matches what the agent prints:</p>
          <CopyBlock text={invite.fingerprint} label="Hub fingerprint" />
          <Waiting key={invite.token} invite={invite} name={name.trim()} />
        </div>
      )}
    </Dialog>
  );
}
