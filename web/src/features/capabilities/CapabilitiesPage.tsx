import { useSelectedHost } from "../../app/HostContext";
import {
  EmptyState,
  InfoTip,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  Text,
} from "../../components";
import { useCapabilities } from "../../api/hooks/capabilities";
import { ChooseHost } from "../skills/ChooseHost";
import { CapabilityList } from "./CapabilityList";

/** Shown when a host's inventory has no rows at all: what that means, and what to check. */
function NoCapabilities({ host }: { host: string }) {
  return (
    <EmptyState icon="tool" title="No capabilities">
      <p>{`The hub knows no capability for ${host}. Check that ${host} is enrolled and its agent is connected on the Hosts page.`}</p>
    </EmptyState>
  );
}

/** The host whose capabilities are shown, with its catalogue state said out loud. */
function Inventory({ host }: { host: string }) {
  const capabilities = useCapabilities(host);
  return (
    <QueryBoundary
      query={capabilities}
      context="load capabilities"
      loading={<Skeleton shape="rows" />}
      isEmpty={(answer) => answer.entries.length === 0}
      empty={<NoCapabilities host={host} />}
    >
      {(answer) => (
        <Stack gap={4}>
          {answer.entries.some((entry) => entry.catalogue.state !== "complete") && (
            <Text muted block>
              This host&apos;s implementation list has not been verified, so a capability that is
              missing from it is not shown as “not installed”.
            </Text>
          )}
          {answer.entries.every((entry) => entry.state !== "available") && (
            <Text muted block>
              Nothing is offered on this host right now. The State column says why for each row.
            </Text>
          )}
          <CapabilityList capabilities={answer.entries} />
          {answer.unknown_contracts.length > 0 && (
            <Text muted size="sm" block>
              The agent also reported{" "}
              <Text mono size="sm">
                {answer.unknown_contracts.join(", ")}
              </Text>
              , which this hub does not know. Nothing is offered for them.
            </Text>
          )}
        </Stack>
      )}
    </QueryBoundary>
  );
}

/**
 * What Domovoi would offer on the selected host, and why not. A read-only view of the hub's
 * own answer: it never enables, disables or approves anything.
 */
export function CapabilitiesPage() {
  const [selected] = useSelectedHost();
  return (
    <>
      <PageHeader
        title="Capabilities"
        docs="concepts/capabilities.html"
        intro={
          <>
            What this hub would offer on a host right now.{" "}
            <InfoTip title="available is not approval">
              A capability the host implements and you enable is available here; whether a
              particular request may run is decided when it is proposed, not on this page.
            </InfoTip>
          </>
        }
      />
      {selected === undefined ? <ChooseHost /> : <Inventory host={selected} />}
    </>
  );
}
