import { useState } from "react";
import { Link } from "react-router";
import { useHosts, useRevoke } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import ui from "../../components/ui.module.css";
import { EnrollDialog } from "./EnrollDialog";
import { HostStatus, VersionCell } from "./HostStatus";
import styles from "./hosts.module.css";

function RevokeButton({ host }: { host: Host }) {
  const [open, setOpen] = useState(false);
  const revoke = useRevoke();
  const close = () => {
    setOpen(false);
    revoke.reset();
  };
  return (
    <>
      <Button variant="danger" onClick={() => setOpen(true)}>
        Revoke
      </Button>
      {open && (
        <ConfirmDialog
          title={`Revoke the agent on ${host.name}?`}
          confirmLabel="Revoke"
          confirmVariant="danger"
          busy={revoke.isPending}
          onConfirm={() => revoke.mutate(host.name, { onSuccess: close })}
          onCancel={close}
        >
          <p>
            The agent is disconnected and its key stops working. To bring the host back, enroll it
            again.
          </p>
          {revoke.error && (
            <p className={styles.error} role="alert">
              Could not revoke the agent. Try again.
            </p>
          )}
        </ConfirmDialog>
      )}
    </>
  );
}

/** Every host with its agent's state, and adding or revoking agents. */
export function HostsPage() {
  const hosts = useHosts();
  const [enrolling, setEnrolling] = useState(false);
  const items = hosts.data ?? [];

  return (
    <>
      <div className={ui.pageHead}>
        <h1>Hosts</h1>
        <Button variant="primary" onClick={() => setEnrolling(true)}>
          Add host
        </Button>
      </div>
      {hosts.isPending && <p>Loading…</p>}
      {hosts.error && <EmptyState title="Could not load hosts" />}
      {hosts.data && items.length === 0 && (
        <EmptyState title="No hosts yet">
          <p>Add a host to enroll its agent.</p>
        </EmptyState>
      )}
      {items.length > 0 && (
        <div className={ui.tableWrap}>
          <table className={ui.table}>
            <thead>
              <tr>
                <th scope="col">Host</th>
                <th scope="col">Kind</th>
                <th scope="col">Agent</th>
                <th scope="col">Version</th>
                <th scope="col">Open incidents</th>
                <th scope="col">
                  <span className="visually-hidden">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((host) => (
                <tr key={host.id}>
                  <th scope="row">
                    <Link to={`/hosts/${host.name}`}>{host.name}</Link>
                  </th>
                  <td>{host.kind}</td>
                  <td>
                    <HostStatus host={host} />
                  </td>
                  <td>
                    <VersionCell host={host} />
                  </td>
                  <td>
                    {host.open_incidents > 0 ? (
                      <Link to={`/incidents?host=${encodeURIComponent(host.name)}`}>
                        {host.open_incidents}
                      </Link>
                    ) : (
                      0
                    )}
                  </td>
                  <td>
                    <RevokeButton host={host} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {enrolling && <EnrollDialog onClose={() => setEnrolling(false)} />}
    </>
  );
}
