import type { CeilingEntry } from "../api/types";
import { Stack } from "../components";
import { HelperKeyPlaceholder } from "../features/capabilities/HelperKeyPlaceholder";
import { HelperPlaceholder } from "../features/capabilities/HelperPlaceholder";
import { LimitsTable } from "../features/capabilities/LimitsTable";
import { Demo, Group } from "./Demo";

const ENTRIES: CeilingEntry[] = [
  {
    contract: "container.restart",
    targets: { kind: "names", names: ["jellyfin", "plex"] },
    max_per_hour: 3,
    max_active: null,
    max_ttl_secs: null,
    args: {},
  },
  {
    contract: "firewall.block_source",
    targets: { kind: "any" },
    max_per_hour: 20,
    max_active: 50,
    max_ttl_secs: 3600,
    args: { min_prefix_len_v4: 24 },
  },
];

/** The Capability Center's own pieces in the catalogue: the host limits table and the two placeholders. */
export function CapabilityDemos() {
  return (
    <Group title="Capability Center">
      <Demo name="Host limits table">
        <LimitsTable host="nas" entries={ENTRIES} />
      </Demo>
      <Demo name="Helper placeholders">
        <Stack gap={4}>
          <HelperPlaceholder />
          <HelperKeyPlaceholder />
        </Stack>
      </Demo>
    </Group>
  );
}
