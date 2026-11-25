import { useHostHelper } from "../../api/hooks/helper";
import { useSelectedHost } from "../../app/HostContext";
import {
  EmptyState,
  LinkButton,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  VisibilityPage,
} from "../../components";
import { HelperKeyEnroll } from "./HelperKeyEnroll";

function ForHost({ host }: { host: string }) {
  const helper = useHostHelper(host);
  return (
    <QueryBoundary
      query={helper}
      context={`load the helper key on ${host}`}
      loading={<Skeleton shape="rows" />}
    >
      {(view) => <HelperKeyEnroll view={view} />}
    </QueryBoundary>
  );
}

/** Where the owner pins a host's helper key, for the host in `?host=` or the selected one. */
export function HelperKeyPage() {
  const [host] = useSelectedHost();
  return (
    <VisibilityPage page="capabilities">
      <PageHeader
        title="Helper key"
        intro="Pin the helper's public key after comparing fingerprints."
      />
      <Stack gap={4}>
        {host === undefined ? (
          <EmptyState icon="server" title="Choose a host">
            <p>A helper key belongs to one host. Choose it on the Helper tab.</p>
          </EmptyState>
        ) : (
          <ForHost host={host} />
        )}
        <div>
          <LinkButton to="/capabilities?tab=helper">Back to the helper tab</LinkButton>
        </div>
      </Stack>
    </VisibilityPage>
  );
}
