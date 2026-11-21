import { useState } from "react";
import { useHosts } from "../../api/hooks/hosts";
import { useManagementPaths } from "../../api/hooks/knowledge";
import { ApiFailure, type ManagementPaths } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  PageHeader,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { AssertFactDialog } from "./AssertFactDialog";
import { HostChooser } from "./HostChooser";
import { PathRows } from "./PathRows";
import { CoverageRow, PathsBanner } from "./PathsBanner";
import styles from "./paths.module.css";

/** The add button: opens the add-a-fact form fixed to a management path of this host. */
function AddPath({ host }: { host: string }) {
  const hosts = useHosts();
  const [open, setOpen] = useState(false);
  const id = hosts.data?.find((candidate) => candidate.name === host)?.id;
  return (
    <>
      <Button
        variant="primary"
        icon="plus"
        disabledReason={id === undefined ? "The host is still loading." : undefined}
        onClick={() => setOpen(true)}
      >
        Add a management path
      </Button>
      {open && id !== undefined && (
        <AssertFactDialog
          onClose={() => setOpen(false)}
          preset={{ predicate: "management_path", subject: `host:${id}` }}
        />
      )}
    </>
  );
}

function HostPaths({ data, host }: { data: ManagementPaths; host: string }) {
  return (
    <div className={styles.stack}>
      <Visible category="P0" id="paths.blocked">
        <PathsBanner
          host={host}
          coverageAvailable={data.coverage_available}
          states={data.paths.map((path) => path.state)}
        />
      </Visible>
      <Visible category="P0" id="paths.coverage">
        <CoverageRow available={data.coverage_available} />
      </Visible>
      <Visible category="P0" id="paths.list">
        {data.paths.length === 0 ? (
          <EmptyState icon="server" title="No management paths yet">
            <p>
              Add the networks you manage this host from. Until then unattended containment stays
              blocked.
            </p>
          </EmptyState>
        ) : (
          <PathRows paths={data.paths} host={host} coverageAvailable={data.coverage_available} />
        )}
      </Visible>
      <Visible category="P1" id="paths.add">
        <div className={styles.add}>
          <AddPath host={host} />
        </div>
      </Visible>
    </div>
  );
}

/**
 * The networks Domovoi must never cut off, host by host, with their state, source, freshness
 * and confirmation, and what is blocked while any of that is missing. The page displays what
 * the hub says; it decides and enforces nothing.
 */
export function PathsPage() {
  const [host, select] = useSelectedHost();
  const paths = useManagementPaths(host);
  const unknownHost =
    paths.data === undefined && paths.error instanceof ApiFailure && paths.error.status === 404;

  const checked =
    host !== undefined && paths.data !== undefined ? (
      <Visible category="P1" id="paths.checked">
        <div className={styles.checked}>
          <Text muted>
            Checked <RelativeTime at={new Date(paths.dataUpdatedAt).toISOString()} />
          </Text>
          <Button icon="refresh" loading={paths.isFetching} onClick={() => void paths.refetch()}>
            Refresh
          </Button>
        </div>
      </Visible>
    ) : undefined;

  return (
    <VisibilityPage page="paths">
      <PageHeader
        title="Management paths"
        intro="The networks you reach each host through. Protective actions must never cut them off."
        docs="concepts/knowledge.html"
        actions={checked}
      />
      {host === undefined ? (
        <Visible category="P1" id="paths.host-chooser">
          <HostChooser onChoose={select} />
        </Visible>
      ) : (
        <Visible category="P0" id="paths.state">
          {unknownHost ? (
            <EmptyState icon="server" title="No such host">
              <p>There is no host named {host}. Pick another host in the selector.</p>
            </EmptyState>
          ) : (
            <QueryBoundary
              query={paths}
              context="load management paths"
              loading={<Skeleton shape="rows" count={3} />}
            >
              {(data) => <HostPaths data={data} host={host} />}
            </QueryBoundary>
          )}
        </Visible>
      )}
    </VisibilityPage>
  );
}
