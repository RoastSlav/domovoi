import type { Capability } from "../../api/types";
import { Cluster, DataTable, StatusPill, Tag, Text, type Column } from "../../components";
import { capabilityStateMeta } from "../../design/tone";
import { catalogueSentence, executionSentence, runnerSentence } from "./capabilityText";

/** One row per capability: what it is, whether it is offered here, and why not. */
const COLUMNS: Column<Capability>[] = [
  {
    key: "contract",
    label: "Capability",
    width: "18rem",
    cell: (capability) => (
      <Cluster gap={2}>
        <Text mono strong>
          {capability.contract}
        </Text>
        {capability.from_hello_skills && <Tag>Skill</Tag>}
      </Cluster>
    ),
  },
  {
    key: "state",
    label: "State",
    width: "11rem",
    cell: (capability) => <StatusPill meta={capabilityStateMeta(capability.state)} />,
  },
  {
    key: "why",
    label: "Why",
    truncate: "clamp2",
    tip: (capability) => `${capability.describe} ${catalogueSentence(capability)}`,
    cell: (capability) => capability.describe,
  },
  {
    key: "effect",
    label: "Effect",
    width: "7rem",
    hint: "The class the hub gives this capability; it decides whether policy treats it as read-only.",
    cell: (capability) => capability.effect,
  },
  {
    key: "execution",
    label: "Runs",
    width: "10rem",
    tip: (capability) =>
      `${executionSentence(capability.execution)}. ${runnerSentence(capability)}`,
    cell: (capability) => executionSentence(capability.execution),
  },
];

/**
 * The capability inventory of one host. Read-only: nothing here enables, disables or
 * approves a capability, and an available row is never a promise that a run will proceed.
 */
export function CapabilityList({ capabilities }: { capabilities: readonly Capability[] }) {
  return (
    <DataTable
      caption="Capabilities on this host"
      columns={COLUMNS}
      rows={capabilities}
      rowKey={(capability) => capability.contract}
    />
  );
}
