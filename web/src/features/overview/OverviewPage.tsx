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
  Visible,
  VisibilityPage,
} from "../../components";
import { hostConnectionOf } from "../../design/tone";
import { HostCards } from "../hosts/HostCards";
import { ApprovalGroup } from "../attention/ApprovalGroup";
import { AttentionRows, PREVIEW_ROWS } from "../attention/AttentionRows";
import { ModelNotice } from "../attention/ModelNotice";
import { SourceFailures } from "../attention/SourceNotices";
import { useAttentionSources } from "../attention/useAttentionSources";
import { RunningNow } from "./RunningNow";
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

const CONNECTION_RANK: Record<string, number> = {
  incompatible: 0,
  offline: 1,
  expected_incompatible: 2,
  never_connected: 3,
};

/** Which hosts to show first: incompatible, offline, expected incompatible, never connected, the rest. */
function hostRank(host: Host): number {
  return CONNECTION_RANK[hostConnectionOf(host)] ?? 4;
}

/** Selects the hosts the section shows: the most urgent state first, then the most open incidents. */
export function hostSeverity(a: Host, b: Host): number {
  return hostRank(a) - hostRank(b) || b.open_incidents - a.open_incidents;
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
          const problems = shown.filter(hasProblem).sort(hostSeverity);
          return (
            <Stack gap={3}>
              {problems.length === 0 ? (
                <Text muted block>
                  {shown.length === 1 ? "The host is fine" : `All ${shown.length} hosts are fine`}
                </Text>
              ) : (
                <HostCards hosts={problems.slice(0, PROBLEM_HOSTS)} />
              )}
              <Visible category="P1" id="control.hosts-link">
                <Link to="/hosts">View all {shown.length} hosts</Link>
              </Visible>
            </Stack>
          );
        }}
      </QueryBoundary>
    </Section>
  );
}

/** The pieces of the Control Room that read the attention sources. */
function NeedsYou({ host }: { host: string | undefined }) {
  const state = useAttentionSources(host);
  const rows = state.items.filter((item) => item.kind !== "approval").slice(0, PREVIEW_ROWS);
  const approvals = state.inputs.waitingApproval.length;
  return (
    <Stack gap={3}>
      <SourceFailures failed={state.failed} />
      {state.pending && state.items.length === 0 && <Skeleton shape="rows" count={3} />}
      {rows.length > 0 && <AttentionRows items={rows} hostNames={state.inputs.hostNames} />}
      {state.allLoaded && rows.length === 0 && (
        <EmptyState
          icon="check-circle"
          title={approvals > 0 ? "Nothing else needs you" : "Nothing needs you"}
        >
          <p>No result is unknown, nothing is paused and no host or incident needs action.</p>
        </EmptyState>
      )}
    </Stack>
  );
}

/** The approval cards, decided in place; the attention centre has the rest. */
function Approvals({ host }: { host: string | undefined }) {
  const state = useAttentionSources(host);
  const { waitingApproval, hostNames } = state.inputs;
  const total = waitingApproval.length;
  const limit = 3;
  if (total === 0) {
    return state.allLoaded ? (
      <EmptyState icon="approve" title="Nothing to approve">
        <p>Actions Domovoi proposes show up here.</p>
      </EmptyState>
    ) : null;
  }
  return (
    <Stack gap={3}>
      <ApprovalGroup
        operations={waitingApproval}
        hostNames={hostNames}
        limit={limit}
        emphasis="secondary"
      />
      {total > limit && <Link to="/attention">{total - limit} more waiting</Link>}
    </Stack>
  );
}

/**
 * The first page of the app, and the answer to "is anything wrong, and what is Domovoi doing?":
 * four counts, what needs the person, what is running, the approvals waiting for a decision
 * and the hosts that need a look. It follows the host selector, like the lists it summarises.
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
    <VisibilityPage page="control-room">
      <PageHeader
        title="Control room"
        intro="What is running, what needs you and how your hosts are doing."
        docs="guide/control-room.html"
      />
      <ModelNotice />
      <div className={styles.tiles}>
        <Visible category="P0" id="control.tiles">
          <QueryBoundary
            query={tiles}
            context="load the control room"
            loading={<OverviewTilesSkeleton />}
          >
            {(shown) => (
              <OverviewTiles overview={shown.overview} hosts={visible(shown.hosts, host)} />
            )}
          </QueryBoundary>
        </Visible>
      </div>
      <div className={styles.columns}>
        <div className={styles.column}>
          <Section
            title="Needs you"
            actions={
              <Link to="/attention" className={styles.viewAll}>
                Attention centre
              </Link>
            }
          >
            <Visible category="P0" id="control.attention">
              <NeedsYou host={host} />
            </Visible>
          </Section>
        </div>
        <div className={styles.column}>
          <Section
            title="Running now"
            actions={
              <Link to="/operations" className={styles.viewAll}>
                All operations
              </Link>
            }
          >
            <Visible category="P0" id="control.running">
              <RunningNow host={host} />
            </Visible>
          </Section>
        </div>
      </div>
      <div className={styles.tiles}>
        <Section title="Waiting for approval">
          <Visible category="P0" id="control.approvals">
            <Approvals host={host} />
          </Visible>
        </Section>
      </div>
      <Visible category="P0" id="control.hosts">
        <HostsSection host={host} hosts={hosts} />
      </Visible>
    </VisibilityPage>
  );
}
