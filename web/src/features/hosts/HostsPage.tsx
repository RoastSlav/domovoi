import { useState } from "react";
import { useHosts } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import {
  Button,
  EmptyState,
  PageHeader,
  QueryBoundary,
  SegmentedControl,
  Skeleton,
  Stack,
  Tag,
} from "../../components";
import { hostConnectionOf } from "../../design/tone";
import { EnrollDialog } from "./EnrollDialog";
import { filterHosts, type HostFilter } from "./hostFilter";
import { HostCards } from "./HostCards";
import { HostsFilters } from "./HostsFilters";
import { HostsTable } from "./HostsTable";
import { useHostsView, type HostsView } from "./useHostsView";

const VIEWS = [
  { value: "cards", label: "Cards" },
  { value: "table", label: "Table" },
] as const;

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
  const [view, setView] = useHostsView();
  const [filter, setFilter] = useState<HostFilter>("all");
  const [query, setQuery] = useState("");
  const add = () => setEnrolling(true);

  return (
    <>
      <PageHeader
        title="Hosts"
        docs="concepts/hub-and-agent.html"
        intro="Machines Domovoi watches, with an agent or through webhooks."
        meta={hosts.data && hosts.data.length > 0 && <Summary hosts={hosts.data} />}
        actions={
          <>
            {hosts.data && hosts.data.length > 0 && (
              <SegmentedControl<HostsView>
                label="Hosts view"
                options={VIEWS}
                value={view}
                onChange={setView}
              />
            )}
            <Button variant="primary" icon="plus" onClick={add}>
              Add host
            </Button>
          </>
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
        {(data) => {
          const shown = filterHosts(data, filter, query);
          return (
            <Stack gap={4}>
              <HostsFilters
                hosts={data}
                filter={filter}
                query={query}
                onFilter={setFilter}
                onQuery={setQuery}
              />
              {shown.length === 0 ? (
                <EmptyState icon="filter" title="No hosts match">
                  <p>Choose All and clear the search to see every host again.</p>
                </EmptyState>
              ) : view === "cards" ? (
                <HostCards hosts={shown} />
              ) : (
                <HostsTable hosts={shown} />
              )}
            </Stack>
          );
        }}
      </QueryBoundary>
      {enrolling && <EnrollDialog onClose={() => setEnrolling(false)} />}
    </>
  );
}
