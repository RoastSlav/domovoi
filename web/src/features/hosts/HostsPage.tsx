import { useState } from "react";
import { Link } from "react-router";
import { useHosts, useRevoke } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  EmptyState,
  LoadError,
  Loading,
  PageHeader,
  useActionFeedback,
} from "../../components";
import ui from "../../components/ui.module.css";
import { EnrollDialog } from "./EnrollDialog";
import { HostStatus, VersionCell } from "./HostStatus";

function RevokeButton({ host }: { host: Host }) {
  const [open, setOpen] = useState(false);
  const revoke = useActionFeedback(useRevoke(), {
    success: (_, name) => `Agent on ${name} revoked`,
    error: "revoke the agent",
    errorDisplay: "inline",
  });
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
          consequence="The agent is disconnected and its key stops working. To bring the host back, enroll it again."
          confirmLabel="Revoke agent"
          tone="danger"
          busy={revoke.isPending}
          error={revoke.error}
          onConfirm={() => revoke.mutate(host.name, { onSuccess: close })}
          onCancel={close}
        />
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
      <PageHeader
        title="Hosts"
        docs="concepts/hub-and-agent.html"
        intro="Machines Domovoi knows about: hosts running an agent, and hosts that webhooks report on."
        actions={
          <Button variant="primary" icon="plus" onClick={() => setEnrolling(true)}>
            Add host
          </Button>
        }
      />
      {hosts.isPending && <Loading />}
      {hosts.error && !hosts.data && (
        <LoadError error={hosts.error} context="load hosts" onRetry={hosts.refetch} />
      )}
      {hosts.data && items.length === 0 && (
        <EmptyState icon="server" title="No hosts yet">
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
                    <Link to={`/hosts/${encodeURIComponent(host.name)}`}>{host.name}</Link>
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
