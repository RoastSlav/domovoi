import { useMemo } from "react";
import { useHostNames } from "../../api/hooks/hosts";
import type { ShadowRow } from "../../api/types";
import {
  DataTable,
  RelativeTime,
  Stack,
  StatusPill,
  Tag,
  Text,
  Visible,
  type Column,
} from "../../components";
import { shadowReasonMeta, shadowStatusMeta } from "../../design/tone";
import { NotEnforced } from "./PolicyNotes";
import { OutcomePill } from "./RuleMarks";
import styles from "./policy.module.css";

/**
 * Recorded decisions as a table whose rows open the decision. The list says once that none of
 * it is enforced.
 * @param rows The recorded decisions to show.
 */
export function DecisionList({ rows }: { rows: readonly ShadowRow[] }) {
  const hostNames = useHostNames();
  const columns = useMemo<Column<ShadowRow>[]>(
    () => [
      {
        key: "step",
        label: "Step",
        cell: (r) => (
          <span className={styles.wrapCell}>
            <Text mono>{r.contract}</Text>
            <Text muted size="xs">
              {hostNames.get(r.host_id) ?? "Unknown host"}
            </Text>
          </span>
        ),
      },
      { key: "at", label: "Evaluated", cell: (r) => <RelativeTime at={r.evaluated_at} /> },
      {
        key: "enforced",
        label: "Active policy",
        cell: (r) =>
          r.status === "evaluated" && r.enforced_outcome ? (
            <OutcomePill outcome={r.enforced_outcome} />
          ) : (
            <StatusPill meta={shadowStatusMeta(r.status)} />
          ),
      },
      {
        key: "shadow",
        label: "Shadow rules",
        cell: (r) => (r.shadow_outcome ? <OutcomePill outcome={r.shadow_outcome} /> : null),
      },
      {
        key: "version",
        label: "Policy",
        cell: (r) => (r.policy_version === 0 ? "no policy" : `version ${r.policy_version}`),
      },
      {
        key: "flags",
        label: "Notes",
        cell: (r) => (
          <span className={styles.marks}>
            {r.degraded && (
              <Tag tone="warning" hint="An input was unavailable when this was decided.">
                Degraded
              </Tag>
            )}
            {r.reason === "late" && <Tag>{shadowReasonMeta("late").label}</Tag>}
          </span>
        ),
      },
    ],
    [hostNames],
  );
  return (
    <Visible category="P0" id="policy.decision-rows">
      <Stack gap={2}>
        <NotEnforced />
        <DataTable
          caption="Recorded decisions"
          columns={columns}
          rows={rows}
          rowKey={(r) => r.execution_id}
          rowHref={(r) => `/policy/decisions/${r.execution_id}`}
        />
      </Stack>
    </Visible>
  );
}
