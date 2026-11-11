import { useSettings } from "../../../api/hooks/system";
import { QueryBoundary, Skeleton, Stack, Text } from "../../../components";
import { Advisories } from "./Advisories";
import { HeartbeatStatus } from "./HeartbeatStatus";
import { Usage } from "./Usage";

/** Model usage, the heartbeat and the open advisories: what the hub is doing, not what it is set to. */
export function StatusSection() {
  const settings = useSettings();
  return (
    <QueryBoundary query={settings} context="load settings" loading={<Skeleton shape="rows" />}>
      {(data) => (
        <Stack gap={5}>
          <Stack gap={2}>
            <Text strong>Model usage</Text>
            <Usage settings={data} />
          </Stack>
          <Stack gap={2}>
            <Text strong>Heartbeat</Text>
            <HeartbeatStatus heartbeat={data.heartbeat} />
          </Stack>
          <Stack gap={2}>
            <Text strong>Advisories</Text>
            <Advisories />
          </Stack>
        </Stack>
      )}
    </QueryBoundary>
  );
}
