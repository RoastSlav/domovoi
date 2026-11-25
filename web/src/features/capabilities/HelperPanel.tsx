import { useHostHelper } from "../../api/hooks/helper";
import { useHosts } from "../../api/hooks/hosts";
import { useSelectedHost } from "../../app/HostContext";
import { EmptyState, QueryBoundary, Skeleton, Stack } from "../../components";
import { HelperHostCard } from "./HelperHostCard";

function ForHost({ name }: { name: string }) {
  const helper = useHostHelper(name);
  return (
    <QueryBoundary
      query={helper}
      context={`load the helper on ${name}`}
      loading={<Skeleton shape="rows" />}
    >
      {(view) => <HelperHostCard view={view} />}
    </QueryBoundary>
  );
}

/**
 * The helper tab: one card per host (or the selected host), each saying whether the hub has
 * a verified report from that host's helper and what it covers. Read only except for Re-check.
 */
export function HelperPanel() {
  const [selected] = useSelectedHost();
  const hosts = useHosts();
  return (
    <QueryBoundary
      query={hosts}
      context="load hosts"
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="server" title="No hosts yet">
          <p>A helper belongs to a host. Add a host on the Hosts page first.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <Stack gap={4}>
          {list
            .filter((host) => selected === undefined || host.name === selected)
            .map((host) => (
              <ForHost key={host.id} name={host.name} />
            ))}
        </Stack>
      )}
    </QueryBoundary>
  );
}
