import { useState } from "react";
import { useHosts } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import { Button, EmptyState, PageHeader, QueryBoundary, Skeleton, Tag } from "../../components";
import { hostConnectionOf } from "../../design/tone";
import { EnrollDialog } from "./EnrollDialog";
import { HostsTable } from "./HostsTable";

/** The page's one-line summary: how many hosts there are and how many need a look. */
function Summary({ hosts }: { hosts: readonly Host[] }) {
  const connections = hosts.map(hostConnectionOf);
  const online = connections.filter((state) => state === "connected").length;
  const offline = connections.filter((state) => state === "offline" || state === "refused").length;
  return (
    <>
      <Tag>{hosts.length === 1 ? "1 host" : `${hosts.length} hosts`}</Tag>
      {online > 0 && <Tag tone="success">{`${online} online`}</Tag>}
      {offline > 0 && <Tag tone="danger">{`${offline} offline`}</Tag>}
    </>
  );
}

/** Every host with its agent's state, and adding or revoking agents. */
export function HostsPage() {
  const hosts = useHosts();
  const [enrolling, setEnrolling] = useState(false);
  const add = () => setEnrolling(true);

  return (
    <>
      <PageHeader
        title="Hosts"
        docs="concepts/hub-and-agent.html"
        intro="Machines Domovoi knows about: hosts running an agent, and hosts that webhooks report on."
        meta={hosts.data && hosts.data.length > 0 && <Summary hosts={hosts.data} />}
        actions={
          <Button variant="primary" icon="plus" onClick={add}>
            Add host
          </Button>
        }
      />
      <AllHostsNote>This is the list the host selector chooses from.</AllHostsNote>
      <QueryBoundary
        query={hosts}
        context="load hosts"
        loading={<Skeleton shape="rows" count={5} />}
        isEmpty={(data) => data.length === 0}
        empty={
          <EmptyState
            icon="server"
            title="No hosts yet"
            action={
              <Button variant="primary" icon="plus" onClick={add}>
                Add your first host
              </Button>
            }
          >
            <p>
              A host is a machine Domovoi watches. Each one runs a small agent that connects to this
              hub and reports in. Add a host to get a one-time command to run on it.
            </p>
          </EmptyState>
        }
      >
        {(data) => <HostsTable hosts={data} />}
      </QueryBoundary>
      {enrolling && <EnrollDialog onClose={() => setEnrolling(false)} />}
    </>
  );
}
