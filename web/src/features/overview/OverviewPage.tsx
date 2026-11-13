import { useHosts } from "../../api/hooks/hosts";
import { useOverview } from "../../api/hooks/overview";
import type { Host } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import { Link } from "react-router";
import {
  EmptyState,
  PageHeader,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Text,
} from "../../components";
import { hostConnectionOf } from "../../design/tone";
import { HostCards } from "../hosts/HostCards";
import { ApprovalList, AttentionList } from "./OverviewLists";
import { OverviewTiles, OverviewTilesSkeleton } from "./OverviewTiles";
import styles from "./overview.module.css";

/** The hosts the selector lets through: all, or the one chosen. */
function visible(hosts: readonly Host[], selected: string | undefined): readonly Host[] {
  return selected ? hosts.filter((one) => one.name === selected) : hosts;
}

/** How many host cards the overview shows; the Hosts page has the rest. */
export const PROBLEM_HOSTS = 6;

/** Whether a host needs a look: not connected (or incompatible), outdated, or with open incidents. */
export function hasProblem(host: Host): boolean {
  const connection = hostConnectionOf(host);
  return (
    (connection !== "connected" && connection !== "hub_only") ||
    host.outdated ||
    host.open_incidents > 0
  );
}

/** Worst first: not connected, then the most open incidents. */
function worst(a: Host, b: Host): number {
  const down = (host: Host) => (host.connected || hostConnectionOf(host) === "hub_only" ? 0 : 1);
  return down(b) - down(a) || b.open_incidents - a.open_incidents;
}

/**
 * The hosts section: only the hosts that need a look (at most six, the worst first), with a
 * link to all of them, or one muted line when every host is fine. A host chosen in the
 * selector is always shown.
 */
function HostsSection({
  host,
  hosts,
}: {
  host: string | undefined;
  hosts: ReturnType<typeof useHosts>;
}) {
  return (
    <Section title="Hosts">
      <QueryBoundary
        query={hosts}
        context="load hosts"
        loading={<Skeleton shape="block" />}
        isEmpty={(all) => all.length === 0}
        empty={
          <EmptyState icon="server" title="No hosts yet">
            <p>Add a host and its agent to start watching it.</p>
          </EmptyState>
        }
      >
        {(all) => {
          const shown = visible(all, host);
          if (host) {
            return <HostCards hosts={shown} />;
          }
          const problems = shown.filter(hasProblem).sort(worst);
          return (
            <Stack gap={3}>
              {problems.length === 0 ? (
                <Text muted block>
                  {shown.length === 1 ? "The host is fine" : `All ${shown.length} hosts are fine`}
                </Text>
              ) : (
                <HostCards hosts={problems.slice(0, PROBLEM_HOSTS)} />
              )}
              <div>
                <Link to="/hosts">View all {shown.length} hosts</Link>
              </div>
            </Stack>
          );
        }}
      </QueryBoundary>
    </Section>
  );
}

/**
 * The first page of the app, and the answer to "is anything wrong?": four counts, the first
 * few incidents that need the person and actions that wait for them, and every host. It
 * follows the host selector, like the lists it summarises.
 */
export function OverviewPage() {
  const [host] = useSelectedHost();
  const overview = useOverview(host);
  const hosts = useHosts();
  // The counts and the host list each can fail or load late; the tiles need both.
  const tiles = {
    data: overview.data && hosts.data ? { overview: overview.data, hosts: hosts.data } : undefined,
    error: overview.error ?? hosts.error,
    isPending: overview.isPending || hosts.isPending,
    refetch: () => {
      void overview.refetch();
      void hosts.refetch();
    },
  };
  return (
    <>
      <PageHeader title="Overview" intro="What needs you and how your hosts are doing." />
      <div className={styles.tiles}>
        <QueryBoundary
          query={tiles}
          context="load the overview"
          loading={<OverviewTilesSkeleton />}
        >
          {(shown) => (
            <OverviewTiles overview={shown.overview} hosts={visible(shown.hosts, host)} />
          )}
        </QueryBoundary>
      </div>
      <div className={styles.columns}>
        <div className={styles.column}>
          <AttentionList host={host} />
        </div>
        <div className={styles.column}>
          <ApprovalList host={host} />
        </div>
      </div>
      <HostsSection host={host} hosts={hosts} />
    </>
  );
}
