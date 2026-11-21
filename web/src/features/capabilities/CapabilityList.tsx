import type { Capability } from "../../api/types";
import {
  Cluster,
  DataTable,
  StatusPill,
  Tag,
  Text,
  Visible,
  visibilityAttrs,
  type Column,
} from "../../components";
import { capabilityStateMeta } from "../../design/tone";
import { catalogueSentence, executionSentence, runnerSentence } from "./capabilityText";

/** One row per capability: what it is, whether it is offered here, and why not. */
const COLUMNS: Column<Capability>[] = [
  {
    key: "contract",
    label: "Capability",
    width: "18rem",
    cell: (capability) => (
      <span {...visibilityAttrs("P1", "capabilities.row.title")}>
        <Cluster gap={2}>
          <Text mono strong>
            {capability.contract}
          </Text>
          {capability.from_hello_skills && <Tag>Skill</Tag>}
        </Cluster>
      </span>
    ),
  },
  {
    key: "state",
    label: "State",
    width: "11rem",
    cell: (capability) => (
      <span {...visibilityAttrs("P0", "capabilities.row.state")}>
        <StatusPill meta={capabilityStateMeta(capability.state)} />
      </span>
    ),
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
    cell: (capability) => (
      <span {...visibilityAttrs("P1", "capabilities.row.title")}>{capability.effect}</span>
    ),
  },
  {
    key: "execution",
    label: "Runs",
    width: "10rem",
    tip: (capability) =>
      `${executionSentence(capability.execution)}. ${runnerSentence(capability)}`,
    cell: (capability) => (
      <span {...visibilityAttrs("P0", "capabilities.row.authority")}>
        {executionSentence(capability.execution)}
        {capability.root_agent || !capability.runner_installed
          ? ` (${runnerSentence(capability)})`
          : ""}
      </span>
    ),
  },
  {
    key: "technical",
    label: "Technical",
    cell: (capability) => (
      <Visible category="P2" id="capabilities.row.technical" label="Technical details">
        <Text size="sm" mono>
          {`v${capability.version} · ${capability.category} · ${capability.target_kind} · ${capability.execution}`}
        </Text>
      </Visible>
    ),
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
