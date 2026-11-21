import { useSelectedHost } from "../../app/HostContext";
import { useCapabilities } from "../../api/hooks/capabilities";
import { EmptyState, QueryBoundary, Skeleton, Stack, Text, Visible } from "../../components";
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
      {(answer) => {
        const unverified = answer.entries.some((entry) => entry.catalogue.state !== "complete");
        const nothing = answer.entries.every((entry) => entry.state !== "available");
        return (
          <Stack gap={4}>
            {(unverified || nothing) && (
              <Visible category="P0" id="capabilities.catalogue.note">
                <Stack gap={4}>
                  {unverified && (
                    <Text muted block>
                      This host&apos;s implementation list has not been verified, so a capability
                      that is missing from it is not shown as “not installed”.
                    </Text>
                  )}
                  {nothing && (
                    <Text muted block>
                      Nothing is offered on this host right now. The State column says why for each
                      row.
                    </Text>
                  )}
                </Stack>
              </Visible>
            )}
            <CapabilityList capabilities={answer.entries} />
            {answer.unknown_contracts.length > 0 && (
              <Visible category="P2" id="capabilities.unknown" label="Unknown contracts">
                <Text muted size="sm" block>
                  The agent also reported{" "}
                  <Text mono size="sm">
                    {answer.unknown_contracts.join(", ")}
                  </Text>
                  , which this hub does not know. Nothing is offered for them.
                </Text>
              </Visible>
            )}
          </Stack>
        );
      }}
    </QueryBoundary>
  );
}

/** The Inventory tab: what the hub would offer on the selected host, and why not. */
export function InventoryTab() {
  const [selected] = useSelectedHost();
  return selected === undefined ? (
    <ChooseHost prompt="Choose a host to see its capabilities." query={{}} />
  ) : (
    <Inventory host={selected} />
  );
}
