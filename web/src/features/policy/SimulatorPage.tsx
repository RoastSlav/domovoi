import { useState } from "react";
import { useSearchParams } from "react-router";
import { useSelectedHost } from "../../app/HostContext";
import { useHosts } from "../../api/hooks/hosts";
import { usePolicyOverview, usePolicyVersions } from "../../api/hooks/policy";
import { useSimulate, type SimulateInput } from "../../api/hooks/policyCommands";
import type { PolicySimulation } from "../../api/types";
import {
  Button,
  Cluster,
  EmptyState,
  LinkButton,
  LoadError,
  PageHeader,
  Skeleton,
  Stack,
  Tag,
  Visible,
  VisibilityPage,
  useActionFeedback,
} from "../../components";
import { SimulatorForm, INITIAL_FILTERS, type FilterValues } from "./SimulatorForm";
import { SimulatorResults, type RunBasis } from "./SimulatorResults";
import { parseSimulatorParams, withSimulatorParams, type SimulatorState } from "./simulatorState";

const HAS_RULES = /^(draft|version-\d+)$/;
const dayStart = (date: string): string => `${date}T00:00:00Z`;

function buildFilter(values: FilterValues, host: string | undefined): SimulateInput["filter"] {
  const filter: SimulateInput["filter"] = {};
  if (host) filter.host = host;
  if (values.contract.trim()) filter.contract = values.contract.trim();
  if (values.status) filter.status = values.status;
  if (values.since) filter.since = dayStart(values.since);
  if (values.until) filter.until = dayStart(values.until);
  return filter;
}

function clampLimit(text: string): number {
  const n = Math.trunc(Number(text));
  return Number.isFinite(n) && n > 0 ? Math.min(n, 500) : 200;
}

/**
 * The policy simulator: re-runs recorded decisions under a draft or a published version and
 * shows, step by step, what was recorded beside what the rules would say. It changes nothing;
 * nothing is sent until Run simulation.
 */
export function SimulatorPage() {
  const overview = usePolicyOverview();
  const versions = usePolicyVersions();
  const hosts = useHosts();
  const [selectedHost] = useSelectedHost();
  const [params, setParams] = useSearchParams();
  const [values, setValues] = useState<FilterValues>(INITIAL_FILTERS);
  const [result, setResult] = useState<{ basis: RunBasis; data: PolicySimulation } | null>(null);
  const simulate = useActionFeedback(useSimulate(), {
    success: () => null,
    error: "run the simulation",
    errorDisplay: "inline",
  });

  if (overview.error) {
    return (
      <LoadError error={overview.error} context="load the policy" onRetry={overview.refetch} />
    );
  }
  const data = overview.data;
  if (!data) {
    return (
      <>
        <PageHeader title="Policy simulator" />
        <Skeleton shape="rows" count={4} />
      </>
    );
  }
  const fallback = data.draft
    ? ({ source: "draft" } as const)
    : data.active
      ? ({ source: "version", number: data.active.number } as const)
      : null;
  const state = parseSimulatorParams(params, fallback ? { rules: fallback } : {});
  const rules = HAS_RULES.test(params.get("rules") ?? "") ? state.rules : fallback;
  const onState = (patch: Partial<SimulatorState>) =>
    setParams(withSimulatorParams(params, patch), { replace: true });
  const pages = versions.data?.pages ?? [];
  const hostId = values.host ?? (hosts.data ?? []).find((h) => h.name === selectedHost)?.id;

  const run = () => {
    if (rules === null) return;
    const basis: RunBasis = { inputs: state.inputs, selection: state.selection };
    simulate.mutate(
      {
        rules,
        selection: state.selection,
        inputs: state.inputs,
        filter: buildFilter(values, hostId),
        execution_ids: state.execution ? [state.execution] : [],
        limit: clampLimit(values.limit),
      },
      { onSuccess: (sim) => setResult({ basis, data: sim }) },
    );
  };

  const header = <PageHeader title="Policy simulator" />;
  if (rules === null) {
    return (
      <VisibilityPage page="policy-simulator">
        {header}
        <Visible category="P0" id="simulator.state">
          <EmptyState
            icon="info"
            title="Nothing to simulate yet"
            action={<LinkButton to="/policy/draft">Start a draft</LinkButton>}
          >
            <p>There is no draft and no published version to run decisions through.</p>
          </EmptyState>
        </Visible>
      </VisibilityPage>
    );
  }
  return (
    <VisibilityPage page="policy-simulator">
      <PageHeader
        title="Policy simulator"
        intro="See what the recorded decisions would have been under other rules. Nothing here changes what runs."
      />
      <Stack gap={5}>
        <SimulatorForm
          state={state}
          rules={rules}
          overview={data}
          versions={pages.flatMap((p) => p.items)}
          moreVersions={Boolean(pages.at(-1)?.next_cursor)}
          values={{ ...values, host: hostId ?? "" }}
          onState={onState}
          onValues={(patch) => setValues({ ...values, ...patch })}
          onRun={run}
          pending={simulate.isPending}
          error={simulate.error}
          step={
            state.execution ? (
              <Cluster gap={2}>
                <Tag>{`Only step ${state.execution.slice(0, 8)}`}</Tag>
                <Button size="sm" onClick={() => onState({ execution: null })}>
                  Show all steps
                </Button>
              </Cluster>
            ) : undefined
          }
        />
        {result && (
          <SimulatorResults
            data={result.data}
            basis={result.basis}
            item={state.item}
            onItem={(item) => onState({ item })}
            recording={data.shadow_enabled}
          />
        )}
      </Stack>
    </VisibilityPage>
  );
}
