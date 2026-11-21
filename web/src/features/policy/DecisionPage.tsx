import { useParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { usePolicyOverview, useShadowDecision } from "../../api/hooks/policy";
import { ApiFailure, type ShadowDetail } from "../../api/types";
import {
  Cluster,
  EmptyState,
  InlineMessage,
  KeyValueList,
  LinkButton,
  LoadError,
  PageHeader,
  Section,
  Skeleton,
  Stack,
  StatusPill,
  Tag,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { shadowReasonMeta, shadowStatusMeta } from "../../design/tone";
import { DecisionExplanation } from "./decision/DecisionExplanation";
import { InputsBlock, StepFacts } from "./decision/DetailBlocks";
import { GapsBlock } from "./decision/GapsBlock";
import { decisionOf } from "./decisionModel";
import { NotEnforced } from "./PolicyNotes";
import { OutcomePill } from "./RuleMarks";
import { useTimeWindowZone } from "./policyZone";

const NO_RECORDING =
  "Decisions are not being recorded. Set shadow to true under [policy] in hub.toml and restart the hub.";
const RECORDING_SOON =
  "It is recorded within a few seconds of the step being created. Try again shortly.";

function Identity({ detail, hostName }: { detail: ShadowDetail; hostName: string }) {
  return (
    <KeyValueList
      items={[
        { label: "Host", value: hostName },
        { label: "Contract", value: detail.contract },
        { label: "Step", value: <Text mono>{detail.execution_id}</Text> },
      ]}
    />
  );
}

function Unevaluated({ detail, hostName }: { detail: ShadowDetail; hostName: string }) {
  return (
    <Visible category="P0" id="decision.state">
      <Stack gap={3}>
        <Cluster gap={2}>
          <StatusPill meta={shadowStatusMeta(detail.status)} />
          {detail.reason && <StatusPill meta={shadowReasonMeta(detail.reason)} />}
        </Cluster>
        <Text block>This step was not evaluated, so there is no decision to explain.</Text>
        <Identity detail={detail} hostName={hostName} />
      </Stack>
    </Visible>
  );
}

function Outcomes({ detail }: { detail: ShadowDetail }) {
  const enforced = detail.enforced_outcome ?? "error";
  const shadow = detail.shadow_outcome ?? "error";
  return (
    <Stack gap={2}>
      <Cluster gap={3}>
        <span>
          <Text muted>Active policy</Text> <OutcomePill outcome={enforced} /> <NotEnforced />
        </span>
        <span>
          <Text muted>Shadow rules</Text> <OutcomePill outcome={shadow} /> <NotEnforced />
        </span>
      </Cluster>
      {enforced !== shadow && (
        <InlineMessage tone="warning">The shadow rules would decide differently.</InlineMessage>
      )}
    </Stack>
  );
}

function Evaluated({ detail, hostName }: { detail: ShadowDetail; hostName: string }) {
  const zone = useTimeWindowZone();
  const doc = decisionOf(detail.shadow);
  const shadow = doc?.kind === "decision" ? doc.doc : null;
  return (
    <Stack gap={5}>
      <Visible category="P1" id="decision.step">
        <Stack gap={2}>
          <Identity detail={detail} hostName={hostName} />
          <StepFacts candidate={detail.candidate} />
        </Stack>
      </Visible>
      <Visible category="P0" id="decision.outcome">
        <Outcomes detail={detail} />
      </Visible>
      <Visible category="P0" id="decision.gaps">
        <GapsBlock gaps={detail.gaps} />
      </Visible>
      <Section title="Active policy">
        <DecisionExplanation
          title="Active policy"
          decision={decisionOf(detail.enforced)}
          zone={zone}
        />
      </Section>
      <Section title="Shadow rules">
        <DecisionExplanation
          title="Shadow rules"
          decision={decisionOf(detail.shadow)}
          zone={zone}
          {...(shadow?.matched.length === 0 ? { emptyText: "No shadow rule matched." } : {})}
        />
      </Section>
      {detail.inputs && (
        <Visible category="P2" id="decision.inputs" label="Recorded inputs">
          <InputsBlock inputs={detail.inputs} />
        </Visible>
      )}
      <Visible category="P3" id="decision.metadata" label="Details" count={4}>
        <KeyValueList
          mono
          items={[
            { label: "Evaluated at", value: detail.evaluated_at },
            { label: "Policy version", value: String(detail.policy_version) },
            {
              label: "Ceiling fingerprint",
              value: shadow?.ceilingHash ?? "not recorded",
            },
            {
              label: "Execution and operation",
              value: `${detail.execution_id} / ${detail.operation_id}`,
            },
          ]}
        />
      </Visible>
    </Stack>
  );
}

/**
 * One recorded decision explained: what the active policy and the shadow rules each said and
 * why, the inputs that were missing, and what was consulted. It only explains; nothing here
 * approves or changes anything.
 */
export function DecisionPage() {
  const { execution } = useParams();
  const query = useShadowDecision(execution);
  const overview = usePolicyOverview();
  const hostNames = useHostNames();
  const detail = query.data;
  if (!detail) {
    if (query.error instanceof ApiFailure && query.error.status === 404) {
      return (
        <VisibilityPage page="policy-decision">
          <PageHeader title="Decision" />
          <Visible category="P0" id="policy.state">
            <EmptyState icon="info" title="No decision recorded for this step">
              <p>
                {overview.data && !overview.data.shadow_enabled ? NO_RECORDING : RECORDING_SOON}
              </p>
            </EmptyState>
          </Visible>
        </VisibilityPage>
      );
    }
    if (query.error) {
      return <LoadError error={query.error} context="load the decision" onRetry={query.refetch} />;
    }
    return (
      <>
        <PageHeader title="Decision" />
        <Skeleton shape="rows" count={4} />
      </>
    );
  }
  const hostName = hostNames.get(detail.host_id) ?? detail.host_id;
  return (
    <VisibilityPage page="policy-decision">
      <PageHeader
        title={`Decision for ${detail.contract}`}
        meta={
          <>
            <StatusPill meta={shadowStatusMeta(detail.status)} />
            <NotEnforced />
            {detail.degraded && <Tag tone="warning">Degraded</Tag>}
          </>
        }
        actions={
          <>
            <LinkButton to={`/operations/${detail.operation_id}?step=${detail.execution_id}`}>
              Open the operation
            </LinkButton>
            <LinkButton to={`/policy/simulator?execution=${detail.execution_id}`}>
              Simulate this step
            </LinkButton>
          </>
        }
      />
      {detail.status === "evaluated" ? (
        <Evaluated detail={detail} hostName={hostName} />
      ) : (
        <Unevaluated detail={detail} hostName={hostName} />
      )}
    </VisibilityPage>
  );
}
