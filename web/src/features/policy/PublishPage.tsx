import { useState } from "react";
import { Link } from "react-router";
import { useDraftPreview, usePolicyDraft, usePolicyOverview } from "../../api/hooks/policy";
import type { PolicyDraft, PolicyPreview } from "../../api/types";
import {
  DetailsScope,
  EmptyState,
  InlineMessage,
  KeyValueList,
  LoadError,
  PageHeader,
  Section,
  Skeleton,
  Stack,
  Tag,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { BlockingFindings, FindingNotes } from "./Findings";
import { LockedChanges } from "./LockedChanges";
import { PolicyNotes } from "./PolicyNotes";
import { PublishControls } from "./PublishControls";
import { SimulationSummary } from "./SimulationSummary";

const FIX_FIRST = "Fix the problems above first.";
const TICK_FIRST = "Acknowledge every conflict above first.";

function Validation({ items }: { items: PolicyPreview["validation"] }) {
  return (
    <InlineMessage tone="danger" title="Fix these rules before publishing">
      <ul>
        {items.map((item) => (
          <li key={`${item.rule ?? ""}:${item.message}`}>
            {item.rule ? (
              <>
                <Link to={`/policy/draft?rule=${encodeURIComponent(item.rule)}`}>{item.rule}</Link>
                {": "}
              </>
            ) : null}
            {item.message}
          </li>
        ))}
      </ul>
    </InlineMessage>
  );
}

function toggled(now: ReadonlySet<string>, key: string, on: boolean): ReadonlySet<string> {
  const next = new Set(now);
  if (on) next.add(key);
  else next.delete(key);
  return next;
}

function Body({ draft, preview }: { draft: PolicyDraft; preview: PolicyPreview }) {
  const [state, setState] = useState<{ basis: string; ticks: ReadonlySet<string> }>({
    basis: preview.preview_hash,
    ticks: new Set(),
  });
  let ticks = state.ticks;
  if (state.basis !== preview.preview_hash) {
    // A new review: keep only the ticks for conflicts it still lists.
    ticks = new Set(preview.blocking.filter((key) => state.ticks.has(key)));
    setState({ basis: preview.preview_hash, ticks });
  }
  const findings = preview.report?.findings ?? [];
  const blocking = findings.filter((finding) => finding.requires_acknowledgement);
  const others = findings.filter((finding) => !finding.requires_acknowledgement);
  const ticked = new Set(preview.blocking.filter((key) => ticks.has(key)));
  const number = preview.base_number + 1;
  const locked = draft.rules.filter((rule) => rule.locked).length;
  const shadow = draft.rules.filter((rule) => rule.shadow).length;
  const summary = preview.simulation;
  let reason: string | undefined;
  if (preview.validation.length > 0) reason = FIX_FIRST;
  else if (ticked.size < preview.blocking.length) reason = TICK_FIRST;
  const tick = (key: string, on: boolean) =>
    setState({ basis: preview.preview_hash, ticks: toggled(ticks, key, on) });

  return (
    <Stack gap={5}>
      <Visible category="P0" id="policy.mode">
        <PolicyNotes notes={preview.notes} />
      </Visible>
      {preview.validation.length > 0 && (
        <Visible category="P0" id="publish.validation">
          <Validation items={preview.validation} />
        </Visible>
      )}
      <Section title="What this publishes">
        <Text block>
          {`Version ${number}: ${draft.rules.length} rules, ${locked} locked, ${shadow} shadow`}
        </Text>
      </Section>
      <Visible category="P0" id="publish.findings">
        <Section title="Conflicts">
          <BlockingFindings findings={blocking} ticked={ticked} onTick={tick} />
        </Section>
      </Visible>
      {others.length > 0 && (
        <Visible category="P1" id="publish.notes">
          <FindingNotes findings={others} />
        </Visible>
      )}
      {preview.locked_changes.length > 0 && (
        <Visible category="P0" id="publish.locks">
          <Section title="Locked rules">
            <LockedChanges changes={preview.locked_changes} />
          </Section>
        </Visible>
      )}
      {summary && (
        <>
          <Visible category="P1" id="publish.simulation">
            <Section title="Simulation">
              <SimulationSummary
                summary={summary}
                label={`Based on the ${summary.evaluated} most recent recorded decisions`}
              />
              <Link to="/policy/simulator?rules=draft">See each decision</Link>
            </Section>
          </Visible>
          {summary.newly_allowed > 0 && (
            <Visible category="P0" id="publish.widening">
              <InlineMessage tone="warning">
                {`${summary.newly_allowed} recorded decisions would become Allow automatically`}
              </InlineMessage>
            </Visible>
          )}
        </>
      )}
      <Visible category="P0" id="publish.controls">
        <PublishControls
          revision={draft.revision}
          previewHash={preview.preview_hash}
          blocking={preview.blocking}
          conflicts={ticked.size}
          lockChanges={preview.locked_changes.length}
          number={number}
          ruleCount={draft.rules.length}
          lockedCount={locked}
          disabledReason={reason}
        />
      </Visible>
      <DetailsScope>
        <Visible category="P3" id="publish.metadata" label="Review details" count={2}>
          <KeyValueList
            mono
            items={[
              { label: "Draft fingerprint", value: preview.rules_hash },
              { label: "Review fingerprint", value: preview.preview_hash },
            ]}
          />
        </Visible>
      </DetailsScope>
    </Stack>
  );
}

function Review({ draft }: { draft: PolicyDraft }) {
  const preview = useDraftPreview(draft);
  let content;
  if (preview.isError) {
    content = (
      <Visible category="P0" id="policy.state">
        <LoadError error={preview.error} context="check the draft" onRetry={preview.refetch} />
      </Visible>
    );
  } else if (preview.data) {
    content = <Body draft={draft} preview={preview.data} />;
  } else {
    content = <Skeleton shape="rows" count={4} />;
  }
  return (
    <>
      <PageHeader
        title="Review and publish"
        meta={
          <>
            <Tag>{`Draft revision ${draft.revision}`}</Tag>
            {draft.based_on != null && <Tag>{`Based on version ${draft.based_on}`}</Tag>}
          </>
        }
        intro="Publishing makes this draft the next version. Read what changes and acknowledge each conflict first."
      />
      {content}
    </>
  );
}

/** The review before a draft becomes the next version, and the password step that publishes it. */
export function PublishPage() {
  const overview = usePolicyOverview();
  const hasDraft = Boolean(overview.data?.draft);
  const draft = usePolicyDraft(hasDraft);
  let content;
  if (overview.isError || draft.isError) {
    content = (
      <Visible category="P0" id="policy.state">
        <LoadError
          error={overview.error ?? draft.error}
          context="load the draft"
          onRetry={overview.isError ? overview.refetch : draft.refetch}
        />
      </Visible>
    );
  } else if (overview.data && !hasDraft) {
    content = (
      <Visible category="P0" id="policy.state">
        <EmptyState
          icon="edit"
          title="No draft to publish"
          action={<Link to="/policy/draft">Go to the draft</Link>}
        >
          <p>Start a draft, change some rules, then come back to publish it.</p>
        </EmptyState>
      </Visible>
    );
  } else if (draft.data) {
    content = <Review draft={draft.data} />;
  } else {
    content = <Skeleton shape="rows" count={4} />;
  }
  return <VisibilityPage page="policy-draft">{content}</VisibilityPage>;
}
