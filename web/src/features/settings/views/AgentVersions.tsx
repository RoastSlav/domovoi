import { useHosts } from "../../../api/hooks/hosts";
import type { Host } from "../../../api/types";
import {
  Cluster,
  KeyValueList,
  QueryBoundary,
  Skeleton,
  Stack,
  Tag,
  Text,
} from "../../../components";

/** One line for all agents: how many run which version, and how many are behind the hub. */
export function summarize(hosts: Host[]): string {
  const reporting = hosts.filter((host) => host.version);
  if (reporting.length === 0) {
    return "No agent is connected, so no version is known.";
  }
  const behind = reporting.filter((host) => host.outdated).length;
  const noun = reporting.length === 1 ? "agent" : "agents";
  return behind === 0
    ? `${reporting.length} ${noun} connected, all up to date.`
    : `${reporting.length} ${noun} connected, ${behind} older than the hub.`;
}

/** The version each connected agent runs, and whether it is behind the hub. */
export function AgentVersions() {
  const hosts = useHosts();
  return (
    <QueryBoundary query={hosts} context="load hosts" loading={<Skeleton shape="rows" count={2} />}>
      {(list) => (
        <Stack gap={3}>
          <Text block>{summarize(list)}</Text>
          <KeyValueList
            mono
            items={list
              .filter((host) => host.version)
              .map((host) => ({
                label: host.name,
                value: (
                  <Cluster gap={2}>
                    <Text mono>{host.version}</Text>
                    {host.outdated && (
                      <Tag
                        tone="warning"
                        hint="This agent is older than the hub. Update it to get the latest checks."
                      >
                        Update available
                      </Tag>
                    )}
                  </Cluster>
                ),
              }))}
          />
        </Stack>
      )}
    </QueryBoundary>
  );
}
