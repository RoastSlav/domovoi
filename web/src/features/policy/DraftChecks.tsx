import { useState } from "react";
import { Link } from "react-router";
import { usePolicyOverview } from "../../api/hooks/policy";
import { usePreviewRules } from "../../api/hooks/policyCommands";
import { ApiFailure, type PolicyDraft, type PolicyPreview, type PolicyRule } from "../../api/types";
import {
  DataTable,
  InlineMessage,
  Stack,
  StatusPill,
  Text,
  Visible,
  useActionFeedback,
  type Column,
} from "../../components";
import { findingKindMeta, lockChangeMeta } from "../../design/tone";
import { PolicyNotes } from "./PolicyNotes";
import type { RuleIssue } from "./ruleIssues";
import { SimulationSummary } from "./SimulationSummary";

/** What checking says in one sentence, so nobody wonders whether it saved. */
export const CHECK_NOTE = "Checking changes nothing. Saving and publishing are separate steps.";

/**
 * Reviews the working copy against the active version without storing anything.
 * @param saved The stored draft; the check is based on its revision.
 * @param working The rules to check.
 * @returns `run`, whether it `pending`, the hub's `result`, the failure `error` and any rule `issues`.
 */
export function useDraftChecks(saved: PolicyDraft, working: PolicyRule[]) {
  const [result, setResult] = useState<PolicyPreview>();
  const [issues, setIssues] = useState<RuleIssue[]>([]);
  const feedback = useActionFeedback(usePreviewRules(), {
    success: () => null,
    error: "check the rules",
    errorDisplay: "inline",
  });
  const run = () => {
    setIssues([]);
    feedback.mutate(
      { baseRevision: saved.revision, rules: working },
      {
        onSuccess: setResult,
        onError: (failure) => setIssues(failure instanceof ApiFailure ? failure.issues : []),
      },
    );
  };
  return { error: feedback.error, issues, pending: feedback.isPending, result, run };
}

function Validation({ preview }: { preview: PolicyPreview }) {
  const columns: Column<PolicyPreview["validation"][number]>[] = [
    {
      key: "rule",
      label: "Rule",
      width: "14rem",
      cell: (entry) =>
        entry.rule ? (
          <Link to={`?rule=${encodeURIComponent(entry.rule)}`}>{entry.rule}</Link>
        ) : (
          <Text muted>Whole draft</Text>
        ),
    },
    { key: "message", label: "Problem", cell: (entry) => entry.message },
  ];
  return (
    <DataTable
      caption="Problems the hub found"
      columns={columns}
      rows={preview.validation}
      rowKey={(entry) => `${entry.rule ?? ""}:${entry.message}`}
      rowTone={() => "danger"}
    />
  );
}

function Findings({ preview }: { preview: PolicyPreview }) {
  const counts = new Map<string, number>();
  for (const finding of preview.report?.findings ?? []) {
    counts.set(finding.kind, (counts.get(finding.kind) ?? 0) + 1);
  }
  return (
    <Stack gap={2}>
      {[...counts].map(([kind, count]) => (
        <div key={kind}>
          <StatusPill meta={findingKindMeta(kind)} />{" "}
          <Text>{count === 1 ? "1 finding" : `${count} findings`}</Text>
        </div>
      ))}
      {counts.size === 0 && <Text muted>The checks found no conflicts between rules.</Text>}
    </Stack>
  );
}

/**
 * What a check found: the problems that stop a save or publish, and below them the findings,
 * the hub's notes, the locked rules that would change and the effect on recorded decisions.
 * @param preview The hub's answer, if the rules were checked.
 * @param error The failure of the check, if it failed.
 */
export function DraftCheckResults({
  preview,
  error,
}: {
  error: ReturnType<typeof useDraftChecks>["error"];
  preview: PolicyPreview | undefined;
}) {
  // The hub's standing notes are on the overview; only what this check adds is shown here.
  const standing = usePolicyOverview().data?.notes ?? [];
  const own = (preview?.notes ?? []).filter((note) => !standing.includes(note));
  if (!preview && !error) return null;
  return (
    <>
      {preview && preview.validation.length > 0 && (
        <Visible category="P0" id="draft.validation">
          <Validation preview={preview} />
        </Visible>
      )}
      <Visible category="P1" id="draft.checks">
        <Stack gap={3}>
          {error && (
            <InlineMessage tone="danger" title={error.title}>
              {error.detail}
            </InlineMessage>
          )}
          {preview && (
            <>
              <Findings preview={preview} />
              {own.length > 0 && <PolicyNotes notes={own} />}
              {preview.locked_changes.map((change) => (
                <div key={`${change.rule}:${change.change}`}>
                  <StatusPill meta={lockChangeMeta(change.change)} />{" "}
                  <Text strong mono>
                    {change.rule}
                  </Text>
                </div>
              ))}
              {preview.simulation && <SimulationSummary summary={preview.simulation} />}
            </>
          )}
          <Text muted block>
            {CHECK_NOTE}
          </Text>
        </Stack>
      </Visible>
    </>
  );
}
