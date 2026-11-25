import type { CeilingEntry } from "../api/types";
import { HelperHostCard } from "../features/capabilities/HelperHostCard";
import { HelperKeyEnroll } from "../features/capabilities/HelperKeyEnroll";
import { LimitsTable } from "../features/capabilities/LimitsTable";
import { helperView, unpinnedView } from "../test/helperFixtures";
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

/** The Capability Center's own pieces in the catalogue: the host limits table and the helper views. */
export function CapabilityDemos() {
  return (
    <Group title="Capability Center">
      <Demo name="Host limits table">
        <LimitsTable host="nas" entries={ENTRIES} />
      </Demo>
      <Demo name="Helper card, verified">
        <HelperHostCard view={helperView()} />
      </Demo>
      <Demo name="Helper card, unverified">
        <HelperHostCard view={unpinnedView()} />
      </Demo>
      <Demo name="Helper key, changed">
        <HelperKeyEnroll view={helperView({ key_state: "changed" })} />
      </Demo>
    </Group>
  );
}
