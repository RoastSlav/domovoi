import { useHostPackages, useRefreshHostPackages } from "../../api/hooks/hostPackages";
import { useHosts } from "../../api/hooks/hosts";
import type { HostPackage } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  DataTable,
  EmptyState,
  InlineMessage,
  LinkButton,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  StatusPill,
  Text,
  Visible,
  visibilityAttrs,
  useActionFeedback,
  type Column,
} from "../../components";
import { hostPackageStatusMeta, keyStateMeta, privilegeBoundaryMeta } from "../../design/tone";
import { ChooseHost } from "../skills/ChooseHost";
import { installOffText, removeHref } from "./InstalledRows";
import styles from "./packages.module.css";

function columns(host: string): Column<HostPackage>[] {
  return [
    {
      key: "package",
      label: "Package",
      cell: (pkg) => (
        <span {...visibilityAttrs("P1", "packages.installed.row")}>
          {pkg.name} <Text mono>{pkg.version}</Text>
        </span>
      ),
    },
    {
      key: "status",
      label: "Status",
      cell: (pkg) => (
        <span {...visibilityAttrs("P0", "packages.installed.status")}>
          <StatusPill meta={hostPackageStatusMeta(pkg.status)} />
        </span>
      ),
    },
    {
      key: "boundary",
      label: "Boundary",
      cell: (pkg) => (
        <span {...visibilityAttrs("P0", "packages.installed.status")}>
          <StatusPill meta={privilegeBoundaryMeta(pkg.privilege_boundary)} />
        </span>
      ),
    },
    {
      key: "key",
      label: "Key",
      cell: (pkg) => (
        <Visible category="P2" id="packages.installed.details" label="Package details">
          <span className={styles.pills}>
            <Text mono>{pkg.key_id}</Text>
            <StatusPill size="sm" meta={keyStateMeta(pkg.key_state)} />
            {pkg.skills.length > 0 && <Text size="sm">{pkg.skills.join(", ")}</Text>}
          </span>
        </Visible>
      ),
    },
    {
      key: "actions",
      label: "Actions",
      hideLabel: true,
      cell: (pkg) => (
        <LinkButton size="sm" to={removeHref(host, pkg)}>
          Remove…
        </LinkButton>
      ),
    },
  ];
}

/** What one host last reported it has installed, with a button that asks its agent again. */
function HostInstalled({ hostId, host }: { host: string; hostId: string }) {
  const installed = useHostPackages(hostId);
  const check = useActionFeedback(useRefreshHostPackages(hostId), {
    error: "check the host",
    errorDisplay: "inline",
    success: `Checked ${host}`,
  });
  const observed = installed.data?.observed_at;
  return (
    <Stack gap={3}>
      <div className={styles.hostHead}>
        <span {...visibilityAttrs("P0", "packages.installed.status")}>
          <Text block>{`What ${host} last reported`}</Text>
          <Text muted size="sm">
            {observed ? (
              <>
                Last checked <RelativeTime at={observed} />
              </>
            ) : (
              "Not checked yet"
            )}
          </Text>
        </span>
        <Button
          icon="refresh"
          hint="Asks the host's agent what is installed now."
          loading={check.isPending}
          onClick={() => check.mutate()}
        >
          Check host
        </Button>
      </div>
      {check.error && (
        <InlineMessage tone="danger" title={check.error.title}>
          {check.error.detail}
        </InlineMessage>
      )}
      <QueryBoundary
        query={installed}
        context="load what is installed"
        loading={<Skeleton shape="rows" />}
        isEmpty={(answer) => answer.entries.length === 0}
        empty={
          <EmptyState icon="package" title="Nothing installed">
            <p>{`Packages you install on ${host} appear here.`}</p>
          </EmptyState>
        }
      >
        {(answer) => (
          <Stack gap={2}>
            {answer.install_allowed === false && (
              <InlineMessage tone="info" title={installOffText(host)} />
            )}
            <DataTable
              caption={`Packages installed on ${host}`}
              columns={columns(host)}
              rows={answer.entries}
              rowKey={(pkg) => `${pkg.name}@${pkg.version}`}
            />
            <Text muted size="sm" block>
              Installed does not mean enabled.
            </Text>
          </Stack>
        )}
      </QueryBoundary>
    </Stack>
  );
}

/** The selected host's installed packages; under "All hosts" it asks for a host. */
export function InstalledOnHost() {
  const [selected] = useSelectedHost();
  const hosts = useHosts();
  if (selected === undefined) {
    return (
      <ChooseHost
        prompt="Choose a host to see what is installed on it."
        query={{ tab: "packages" }}
      />
    );
  }
  const host = hosts.data?.find((candidate) => candidate.name === selected);
  return (
    <QueryBoundary query={hosts} context="load hosts" loading={<Skeleton shape="rows" />}>
      {() =>
        host ? (
          <HostInstalled host={host.name} hostId={host.id} />
        ) : (
          <EmptyState icon="server" title="No such host" />
        )
      }
    </QueryBoundary>
  );
}
