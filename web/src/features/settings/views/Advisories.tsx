import { useHostNames } from "../../../api/hooks/hosts";
import { useAdvisories } from "../../../api/hooks/system";
import {
  Card,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  Text,
} from "../../../components";

/** The open advisories, with the host each is about. */
export function Advisories() {
  const advisories = useAdvisories();
  const hosts = useHostNames();
  return (
    <QueryBoundary
      query={advisories}
      context="load advisories"
      loading={<Skeleton shape="rows" count={2} />}
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="check-circle" title="No open advisories">
          <p>Nothing the hub has noticed needs your attention.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <Stack gap={3}>
          {list.map((advisory) => (
            <Card key={advisory.id} tone="warning">
              <Stack gap={1}>
                <Text strong>{advisory.title}</Text>
                <Text muted size="sm">
                  {advisory.host_id ? `${hosts.get(advisory.host_id) ?? "unknown host"} · ` : ""}
                  <RelativeTime at={advisory.created_at} />
                </Text>
                <Text block>{advisory.detail}</Text>
              </Stack>
            </Card>
          ))}
        </Stack>
      )}
    </QueryBoundary>
  );
}
