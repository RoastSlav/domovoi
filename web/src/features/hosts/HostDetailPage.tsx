import { Link, useParams, useSearchParams } from "react-router";
import { useHost } from "../../api/hooks/hosts";
import { ApiFailure, type HostDetail } from "../../api/types";
import {
  EmptyState,
  InlineMessage,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  Tabs,
  type Tab,
} from "../../components";
import { ChecksTab } from "./ChecksTab";
import { useFailingChecks } from "./checkState";
import { HostActions } from "./HostActions";
import { HostMeta, isHubOnly } from "./HostMeta";
import { HostOverview } from "./HostOverview";
import { LogsTab } from "./LogsTab";
import { ProfileTab } from "./ProfileTab";
import { SshLoginsTab } from "./SshLoginsTab";

/** The tabs a host's page has, by the value `?tab=` holds; the first is the default. */
const TAB_IDS = ["overview", "profile", "checks", "logs", "ssh"] as const;

/** Why the hub's own machine has no checks, and how to give it some. */
function HubOnlyNote({ name }: { name: string }) {
  return (
    <InlineMessage tone="info" title="This is the hub’s own machine.">
      <p>
        Domovoi records hub problems here, such as the hub losing its network. To also watch this
        machine&rsquo;s disks, services and containers, install an agent on it and enroll it under
        the name <strong>{name}</strong>. The hub can also run the agent in its own process with{" "}
        <code>--with-local-agent /etc/domovoi/agent.toml</code>, where checks that need root report
        errors.
      </p>
    </InlineMessage>
  );
}

/** The host's tabs, the open one kept in the URL so a link or a reload lands on it. */
function HostTabs({ detail }: { detail: HostDetail }) {
  const [params, setParams] = useSearchParams();
  const failing = useFailingChecks(detail.host.name);
  const name = detail.host.name;
  const asked = params.get("tab");
  const selected = TAB_IDS.find((id) => id === asked) ?? TAB_IDS[0];
  const tabs: Tab[] = [
    {
      id: "overview",
      label: "Overview",
      content: <HostOverview name={name} facts={detail.facts} />,
    },
    {
      id: "profile",
      label: "Profile",
      content: <ProfileTab host={name} connected={detail.host.connected} />,
    },
    {
      id: "checks",
      label: "Checks",
      count: { value: failing.count, label: "failing", tone: "danger" },
      content: <ChecksTab host={name} />,
    },
    { id: "logs", label: "Logs", content: <LogsTab host={name} /> },
    { id: "ssh", label: "SSH logins", content: <SshLoginsTab host={name} /> },
  ];
  return (
    <Tabs
      label={`${name} details`}
      tabs={tabs}
      selected={selected}
      onSelect={(id) =>
        setParams(
          (now) => {
            const next = new URLSearchParams(now);
            if (id === TAB_IDS[0]) {
              next.delete("tab");
            } else {
              next.set("tab", id);
            }
            return next;
          },
          { replace: true },
        )
      }
    />
  );
}

/** One host: its agent, its facts, incidents and skills, its profile, checks, logs and logins. */
export function HostDetailPage() {
  const { name = "" } = useParams();
  const host = useHost(name);
  if (host.error instanceof ApiFailure && host.error.status === 404 && !host.data) {
    return (
      <EmptyState
        icon="server"
        title="No such host"
        action={<Link to="/hosts">Back to hosts</Link>}
      >
        <p>It may have been removed, or the address is wrong.</p>
      </EmptyState>
    );
  }
  return (
    <QueryBoundary
      query={host}
      context="load the host"
      loading={<Skeleton shape="rows" count={5} />}
    >
      {(detail) => (
        <Stack gap={5}>
          <PageHeader
            title={detail.host.name}
            meta={<HostMeta host={detail.host} />}
            actions={<HostActions host={detail.host} />}
          />
          {isHubOnly(detail.host) && <HubOnlyNote name={detail.host.name} />}
          <HostTabs detail={detail} />
        </Stack>
      )}
    </QueryBoundary>
  );
}
