import type { IncidentDetail, TimelineEntry } from "../../api/types";
import {
  DataTable,
  RelativeTime,
  Section,
  Stack,
  Text,
  Timeline,
  type Column,
} from "../../components";
import { timelineMeta } from "../../design/timeline";

type ToolUsage = IncidentDetail["tools_used"][number];

const TOOL_COLUMNS: Column<ToolUsage>[] = [
  { key: "name", label: "Name", cell: (tool) => <Text mono>{tool.name}</Text> },
  { key: "kind", label: "Kind", cell: (tool) => tool.kind },
  { key: "calls", label: "Calls", numeric: true, cell: (tool) => tool.calls },
  { key: "failures", label: "Failures", numeric: true, cell: (tool) => tool.failures },
  {
    key: "refused",
    label: "Not run",
    numeric: true,
    hint: "Calls the model asked for after its tool budget for the run was spent.",
    cell: (tool) => tool.refused,
  },
  { key: "last", label: "Last used", cell: (tool) => <RelativeTime at={tool.last_used} /> },
];

/** What triage and the follow-up chat looked at, with how often, and the skills it proposed. */
export function ToolsUsed({ tools, proposed }: { proposed: string[]; tools: ToolUsage[] }) {
  return (
    <Section
      title="Skills and tools used"
      description="Read tools and read skills only look; they never change the host."
    >
      {tools.length === 0 && proposed.length === 0 ? (
        <Text block muted size="sm">
          Nothing used yet.
        </Text>
      ) : (
        <Stack gap={2}>
          {tools.length > 0 && (
            <DataTable
              caption="Skills and tools used"
              columns={TOOL_COLUMNS}
              rows={tools}
              rowKey={(tool) => `${tool.kind}-${tool.name}`}
            />
          )}
          {proposed.length > 0 && (
            <Text block size="sm">
              Skills proposed: {proposed.join(", ")}.
            </Text>
          )}
        </Stack>
      )}
    </Section>
  );
}

/** Everything that happened to the incident, in words, oldest first. */
export function IncidentTimeline({ entries }: { entries: TimelineEntry[] }) {
  return (
    <Section
      title="Timeline"
      description="Everything that happened to this incident, oldest first."
    >
      {entries.length === 0 ? (
        <Text block muted size="sm">
          Nothing yet.
        </Text>
      ) : (
        <Timeline
          label="Timeline"
          items={entries.map((entry) => ({ at: entry.at, meta: timelineMeta(entry) }))}
        />
      )}
    </Section>
  );
}
