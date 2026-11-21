import type { PolicyFinding } from "../../api/types";
import { Card, Checkbox, InlineMessage, Stack, StatusPill, Tag, Text } from "../../components";
import { findingKindMeta, policyOutcomeMeta, ruleMarkMeta } from "../../design/tone";
import { OutcomePill } from "./RuleMarks";

/** The rules a finding is about, in words. */
function pair(finding: PolicyFinding): string {
  return finding.first === finding.second
    ? finding.first
    : `${finding.first} and ${finding.second}`;
}

/** The sentence beside a finding's box. */
function acknowledgement(finding: PolicyFinding): string {
  const resolution = finding.resolution
    ? ` resolve to ${policyOutcomeMeta(finding.resolution).label}`
    : "";
  return `Acknowledge: ${pair(finding)}${resolution}`;
}

function FindingCard({
  finding,
  ticked,
  onTick,
}: {
  finding: PolicyFinding;
  ticked: boolean;
  onTick: (key: string, on: boolean) => void;
}) {
  return (
    <Card tone="warning">
      <Stack gap={2}>
        <Stack gap={1}>
          <StatusPill meta={findingKindMeta(finding.kind)} />
          <Text block>{pair(finding)}</Text>
          {finding.resolution && (
            <Text block>
              Resolves to <OutcomePill outcome={finding.resolution} />
            </Text>
          )}
          {finding.shadow && <Tag>{ruleMarkMeta("shadow").label}</Tag>}
        </Stack>
        <Text muted block>
          {finding.note}
        </Text>
        <Checkbox
          label={acknowledgement(finding)}
          hint="You must acknowledge every conflict to publish."
          checked={ticked}
          onChange={(on) => onTick(finding.key, on)}
        />
      </Stack>
    </Card>
  );
}

/**
 * The conflicts a review found that must each be acknowledged, one box each.
 * @param findings Only those the hub requires an acknowledgement for.
 * @param ticked Keys of the boxes ticked now.
 * @param onTick Called when a box is ticked or cleared.
 */
export function BlockingFindings({
  findings,
  ticked,
  onTick,
}: {
  findings: readonly PolicyFinding[];
  onTick: (key: string, on: boolean) => void;
  ticked: ReadonlySet<string>;
}) {
  if (findings.length === 0) {
    return <InlineMessage tone="success">No conflicts found.</InlineMessage>;
  }
  return (
    <Stack gap={3}>
      {findings.map((finding) => (
        <FindingCard
          key={finding.key}
          finding={finding}
          ticked={ticked.has(finding.key)}
          onTick={onTick}
        />
      ))}
    </Stack>
  );
}

/**
 * Findings that need no acknowledgement, as a compact list.
 * @param findings Those the hub does not require an acknowledgement for.
 */
export function FindingNotes({ findings }: { findings: readonly PolicyFinding[] }) {
  return (
    <Stack gap={1}>
      {findings.map((finding) => (
        <Text key={finding.key} block muted>
          <StatusPill size="sm" meta={findingKindMeta(finding.kind)} /> {finding.note}
        </Text>
      ))}
    </Stack>
  );
}
