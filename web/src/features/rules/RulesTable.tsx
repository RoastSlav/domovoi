import type { AckRule } from "../../api/types";
import {
  Cluster,
  Countdown,
  DataTable,
  RelativeTime,
  Stack,
  Tag,
  Text,
  useRemaining,
  type Column,
} from "../../components";
import { matchPhrase } from "./phrase";
import { RuleRowMenu } from "./RuleRowMenu";

/** Within this long of its end a rule's expiry counts down. */
const COUNTDOWN_WITHIN_MS = 3_600_000;

/** A deadline close enough to tick. */
function Ticking({ at }: { at: string }) {
  const remaining = useRemaining(at);
  return (
    <Cluster gap={1}>
      <Text muted>in</Text>
      <Countdown remaining={remaining} until={at} />
    </Cluster>
  );
}

/** When a rule stops applying: "Always", a countdown, a relative time, or "Expired". */
function Expires({ at }: { at: string | null | undefined }) {
  if (!at) {
    return <Tag>Always</Tag>;
  }
  const ends = new Date(at).getTime();
  const left = ends - new Date().getTime();
  if (Number.isFinite(ends) && left <= 0) {
    return (
      <Cluster gap={2}>
        <Tag>Expired</Tag>
        <RelativeTime at={at} />
      </Cluster>
    );
  }
  return Number.isFinite(ends) && left < COUNTDOWN_WITHIN_MS ? (
    <Ticking at={at} />
  ) : (
    <RelativeTime at={at} />
  );
}

/** Whether the rule has already run out. */
function isExpired(rule: AckRule): boolean {
  return (
    Boolean(rule.expires_at) && new Date(rule.expires_at ?? "").getTime() <= new Date().getTime()
  );
}

const COLUMNS: Column<AckRule>[] = [
  {
    key: "matches",
    label: "What it matches",
    hint: "The check, its target and the conditions an incident must meet. An incident that matches is kept in the history and never notifies.",
    cell: (rule) => (
      <Stack gap={1}>
        <span>{matchPhrase({ ...rule, target: rule.target_pattern })}</span>
        <Text muted size="xs" block>
          {rule.note}
        </Text>
      </Stack>
    ),
  },
  {
    key: "host",
    label: "Host",
    cell: (rule) => (rule.host ? <span>{rule.host}</span> : <Tag>Any host</Tag>),
  },
  { key: "expires", label: "Expires", cell: (rule) => <Expires at={rule.expires_at} /> },
  {
    key: "actions",
    label: "Actions",
    hideLabel: true,
    cell: (rule) => <RuleRowMenu rule={rule} />,
  },
];

/** The known conditions as a table, soonest expiry first as the hub sends them. */
export function RulesTable({ rules }: { rules: readonly AckRule[] }) {
  return (
    <DataTable
      caption="Known conditions"
      columns={COLUMNS}
      rows={rules}
      rowKey={(rule) => rule.id}
      rowState={(rule) => (isExpired(rule) ? "quiet" : undefined)}
    />
  );
}
