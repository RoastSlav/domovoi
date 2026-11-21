import { useEffect, useState } from "react";
import { useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { usePolicyDraft, usePolicyOverview } from "../../api/hooks/policy";
import type { PolicyDraft, PolicyOverview, PolicyRule } from "../../api/types";
import {
  EmptyState,
  InfoTip,
  InlineMessage,
  LinkButton,
  PageHeader,
  QueryBoundary,
  Stack,
  SubmitBar,
  Tag,
  Visible,
  VisibilityPage,
} from "../../components";
import { useLeaveGuard } from "../notes/useLeaveGuard";
import { DiscardDraft } from "./DiscardDraft";
import { DraftActions } from "./DraftActions";
import { DraftBanner } from "./DraftBanner";
import { DraftDetails } from "./DraftDetails";
import { DraftConfirms } from "./DraftConfirms";
import { DraftCheckResults, useDraftChecks } from "./DraftChecks";
import { PRECEDENCE_HELP } from "./policyText";
import { useTimeWindowZone } from "./policyZone";
import { RuleEditor } from "./RuleEditor";
import { RulesTable } from "./RulesTable";
import { StartDraft } from "./StartDraft";
import { useDraftState } from "./useDraftState";
import { useSaveDraftFlow } from "./useSaveDraftFlow";
import styles from "./policy.module.css";

/** More unsaved changes than this ask before Discard throws them away. */
const CONFIRM_DISCARD_FROM = 3;

function DraftEditor({ fetched, overview }: { fetched: PolicyDraft; overview: PolicyOverview }) {
  const [params, setParams] = useSearchParams();
  const hostNames = useHostNames();
  const zone = useTimeWindowZone();
  const state = useDraftState(fetched);
  const { saved, working, dirty, changes } = state;
  const flow = useSaveDraftFlow({ saved, working, onSaved: state.adopt });
  const checks = useDraftChecks(saved, working);
  const { guard, allowLeaving } = useLeaveGuard(dirty, "draft", true);
  const [discarding, setDiscarding] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [removing, setRemoving] = useState<PolicyRule>();

  const wanted = params.get("rule");
  const open = wanted === "new" || (wanted !== null && working.some((r) => r.id === wanted));
  const openRule = (id: string | null) =>
    setParams(
      (now) => {
        const next = new URLSearchParams(now);
        if (id === null) next.delete("rule");
        else next.set("rule", id);
        return next;
      },
      { replace: id === null },
    );
  useEffect(() => {
    if (wanted !== null && !open) openRule(null);
    // openRule only reads the setter; the URL is the input.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted, open]);

  const elsewhere = dirty && state.staleRevision && !flow.saving;
  const stale = (overview.draft?.based_on ?? null) !== (overview.active?.number ?? null);
  const onRemove = (id: string) => {
    const rule = working.find((r) => r.id === id);
    if (rule?.locked) setRemoving(rule);
    else state.remove(id);
  };
  const body = (
    <Stack gap={4}>
      <Visible category="P0" id="draft.rules">
        <RulesTable
          mode="edit"
          rules={working}
          hostNames={hostNames}
          zone={zone}
          selectedId={open ? (wanted ?? undefined) : undefined}
          onEdit={openRule}
          onDuplicate={(id) => {
            const copy = state.duplicate(id);
            if (copy) openRule(copy);
          }}
          onRemove={onRemove}
        />
      </Visible>
      <DraftCheckResults preview={checks.result} error={checks.error} />
    </Stack>
  );

  return (
    <Stack gap={5}>
      <PageHeader
        title={
          <>
            Policy draft <InfoTip title="how rules combine">{PRECEDENCE_HELP}</InfoTip>
          </>
        }
        meta={
          <>
            <Tag>{`Revision ${saved.revision}`}</Tag>
            <Tag>
              {saved.based_on == null
                ? "Based on no version"
                : `Based on version ${saved.based_on}`}
            </Tag>
            {dirty && <Tag tone="warning">Unsaved changes</Tag>}
          </>
        }
        actions={
          <DraftActions
            dirty={dirty}
            checking={checks.pending}
            onCheck={checks.run}
            onDiscard={() => setDiscarding(true)}
          />
        }
      />
      {stale && (
        <Visible category="P0" id="policy.draft-stale">
          <InlineMessage
            tone="warning"
            title="The active version changed since this draft began"
            action={
              <LinkButton to="/policy/simulator?rules=draft" size="sm">
                Simulate the draft
              </LinkButton>
            }
          >
            Review the draft before you publish. Saving it again re-bases it.
          </InlineMessage>
        </Visible>
      )}
      <DraftBanner
        dirty={dirty}
        changes={changes}
        elsewhere={elsewhere}
        issues={[...flow.issues, ...checks.issues]}
        onReload={() => setReloading(true)}
      />
      {open && wanted ? (
        <div className={styles.editorLayout}>
          {body}
          <Visible category="P1" id="draft.editor">
            <RuleEditor
              key={wanted}
              ruleId={wanted}
              rules={working}
              onApply={state.apply}
              onClose={() => openRule(null)}
            />
          </Visible>
        </div>
      ) : (
        body
      )}
      <DraftDetails draft={saved} />
      <SubmitBar
        changes={changes}
        saveLabel="Save draft"
        saving={flow.saving}
        disabledReason={elsewhere ? "Reload the draft first." : undefined}
        onSave={flow.onSave}
        onDiscard={() => (changes > CONFIRM_DISCARD_FROM ? setDropping(true) : state.reset())}
      />
      {flow.dialog}
      {guard}
      {discarding && (
        <DiscardDraft
          revision={saved.revision}
          allowLeaving={allowLeaving}
          onClose={() => setDiscarding(false)}
        />
      )}
      <DraftConfirms
        changes={changes}
        dropping={dropping}
        reloading={reloading}
        removing={removing}
        onDrop={() => {
          state.reset();
          setDropping(false);
        }}
        onReload={() => {
          state.reload();
          flow.clearIssues();
          setReloading(false);
        }}
        onRemove={(id) => {
          state.remove(id);
          setRemoving(undefined);
        }}
        onClose={() => {
          setDropping(false);
          setReloading(false);
          setRemoving(undefined);
        }}
      />
    </Stack>
  );
}

function LoadedDraft({ overview }: { overview: PolicyOverview }) {
  const draft = usePolicyDraft(true);
  return (
    <QueryBoundary query={draft} context="load the draft">
      {(fetched) => <DraftEditor fetched={fetched} overview={overview} />}
    </QueryBoundary>
  );
}

/**
 * The policy draft: its rules as a table, the rule editor beside it, the checks and the
 * floating bar that saves. Edits live on the page until the draft is saved.
 */
export function DraftPage() {
  const overview = usePolicyOverview();
  return (
    <VisibilityPage page="policy-draft">
      <QueryBoundary query={overview} context="load the policy">
        {(data) =>
          data.draft ? (
            <LoadedDraft overview={data} />
          ) : (
            <Stack gap={5}>
              <PageHeader title="Policy draft" />
              <Visible category="P0" id="policy.state">
                <EmptyState
                  icon="edit"
                  title="There is no draft"
                  action={<StartDraft from={data.active?.number ?? null} />}
                >
                  Start one from the active version, or from nothing.
                </EmptyState>
              </Visible>
            </Stack>
          )
        }
      </QueryBoundary>
    </VisibilityPage>
  );
}
