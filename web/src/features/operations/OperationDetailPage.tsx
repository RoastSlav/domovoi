import { useState } from "react";
import { Link, useParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useOperation } from "../../api/hooks/operations";
import { ApiFailure } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import { usePageName } from "../../app/PageTitle";
import {
  EmptyState,
  InlineMessage,
  LoadError,
  PageHeader,
  Progress,
  SegmentedControl,
  Section,
  Skeleton,
  Stack,
  Visible,
  VisibilityPage,
} from "../../components";
import { CancelDialog } from "./CancelDialog";
import { OperationBanner } from "./OperationBanner";
import { OperationDetails } from "./OperationDetails";
import { OperationHeader } from "./OperationHeader";
import { StaleNote } from "./StaleNote";
import { StepInspector } from "./StepInspector";
import { StepOutline, type OutlineView } from "./StepOutline";
import { useStepSelection } from "./useStepSelection";
import { WaitingForYou, focusWaiting } from "./WaitingForYou";

const VIEWS = [
  { value: "timeline", label: "Timeline" },
  { value: "tree", label: "Tree" },
] as const;

function name(goal: string | undefined): string | undefined {
  return goal && goal.length > 60 ? `${goal.slice(0, 59)}…` : goal;
}

/**
 * One operation: its header and controls, the one banner that needs attention, what waits
 * for a person, the steps as a timeline or tree, and its details. A step opens in the
 * inspector in the right-hand slot. The page follows the shell's journal stream: a frame
 * newer than the page's `as_of_seq` refreshes it.
 */
export function OperationDetailPage() {
  const { id = "" } = useParams();
  const query = useOperation(id);
  const hostNames = useHostNames();
  const data = query.data;
  const { selected, select, clear } = useStepSelection(data?.trace);
  const [view, setView] = useState<OutlineView>("timeline");
  const [cancelling, setCancelling] = useState(false);
  usePageName(name(data?.operation.goal));

  if (!data) {
    if (query.error instanceof ApiFailure && query.error.status === 404) {
      return (
        <EmptyState
          icon="play"
          title="No such operation"
          action={<Link to="/operations">Back to Operations</Link>}
        >
          <p>It may have been removed, or the address is wrong.</p>
        </EmptyState>
      );
    }
    return query.error ? (
      <LoadError error={query.error} context="load the operation" onRetry={query.refetch} />
    ) : (
      <>
        <PageHeader title="Operation" />
        <Skeleton shape="rows" count={5} />
      </>
    );
  }

  const { operation, trace, executions } = data;
  const running = trace.steps.find((step) => step.outcome === "running");
  return (
    <VisibilityPage page="operation">
      <OperationHeader
        operation={operation}
        hostNames={hostNames}
        onCancel={() => setCancelling(true)}
      />
      <Stack gap={5}>
        {query.isError && (
          <Visible category="P0" id="operation.stale-note">
            <StaleNote updatedAt={query.dataUpdatedAt} onRetry={() => void query.refetch()} />
          </Visible>
        )}
        <Visible category="P1" id="operation.scope-note">
          <AllHostsNote>An operation is shown whatever host is selected.</AllHostsNote>
        </Visible>
        {trace.schema_version > 1 && (
          <Visible category="P0" id="operation.schema-warning">
            <InlineMessage tone="warning">
              This page was built for an older operation format. Some details may be missing.
            </InlineMessage>
          </Visible>
        )}
        <Visible category="P0" id="operation.banner">
          <OperationBanner
            operation={operation}
            trace={trace}
            executions={executions}
            hostNames={hostNames}
            onShowStep={(step) => select(step.id, document.body)}
            onReview={focusWaiting}
            onCancel={() => setCancelling(true)}
          />
        </Visible>
        {operation.status === "running" && (
          <Visible category="P0" id="operation.progress">
            <Progress label={running?.summary ? `Working: ${running.summary}` : "Working…"} />
          </Visible>
        )}
        {operation.status === "queued" && (
          <Visible category="P0" id="operation.progress">
            <Progress label="Queued. It starts when a slot is free." />
          </Visible>
        )}
        <WaitingForYou
          operationId={operation.id}
          trace={trace}
          executions={executions}
          hostNames={hostNames}
          paused={operation.status === "paused"}
        />
        <Section
          title="Steps"
          actions={
            <Visible category="P1" id="operation.steps-head">
              <SegmentedControl label="View" options={VIEWS} value={view} onChange={setView} />
            </Visible>
          }
        >
          <StepOutline
            trace={trace}
            executions={executions}
            selected={selected}
            onSelect={select}
            view={view}
          />
        </Section>
        <Section title="Details">
          <OperationDetails trace={trace} hostNames={hostNames} />
        </Section>
      </Stack>
      <StepInspector
        operationId={operation.id}
        trace={trace}
        executions={executions}
        hostNames={hostNames}
        selected={selected}
        onSelect={select}
        onClose={clear}
        onReview={focusWaiting}
      />
      {cancelling && (
        <CancelDialog
          operationId={operation.id}
          goal={operation.goal}
          onClose={() => setCancelling(false)}
        />
      )}
    </VisibilityPage>
  );
}
