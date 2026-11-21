import type { PolicyAck } from "../../api/types";
import { Cluster, DataTable, RelativeTime, StatusPill, Text, type Column } from "../../components";
import { findingKindMeta } from "../../design/tone";
import { OutcomePill } from "./RuleMarks";
import { actorText } from "./ruleText";

const COLUMNS: Column<PolicyAck>[] = [
  {
    key: "kind",
    label: "Conflict",
    cell: (a) => <StatusPill size="sm" meta={findingKindMeta(a.kind)} />,
  },
  {
    key: "rules",
    label: "Rules",
    cell: (a) => (
      <Cluster gap={1}>
        <Text mono>{a.first_rule}</Text>
        <span>and</span>
        <Text mono>{a.second_rule}</Text>
      </Cluster>
    ),
  },
  {
    key: "resolution",
    label: "Resolved to",
    cell: (a) => (a.resolution ? <OutcomePill outcome={a.resolution} /> : null),
  },
  {
    key: "who",
    label: "Acknowledged by",
    cell: (a) => (
      <>
        {actorText(a.acknowledged_by)} <RelativeTime at={a.acknowledged_at} />
      </>
    ),
  },
];

/**
 * The conflicts a person acknowledged when this version was published.
 * @param items The acknowledgements stored with the version.
 */
export function VersionAcks({ items }: { items: readonly PolicyAck[] }) {
  if (items.length === 0) {
    return <Text muted>No conflicts needed acknowledging for this version.</Text>;
  }
  return (
    <DataTable
      caption="Acknowledged conflicts"
      columns={COLUMNS}
      rows={items}
      rowKey={(a) => a.finding_key}
    />
  );
}
