import { useMemo, useState } from "react";
import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import type { Action } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  EmptyState,
  PageHeader,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Tag,
} from "../../components";
import { ActionCard } from "./ActionCard";
import { useActionOutcomes } from "./useActionOutcomes";

/** How far back "Recently decided" looks. */
const RECENT_MS = 24 * 60 * 60 * 1000;

/** `actions` with the one that expires soonest first. */
function byExpiry(actions: readonly Action[]): Action[] {
  return [...actions].sort((a, b) => Date.parse(a.expires_at) - Date.parse(b.expires_at));
}

function Cards({ actions, hosts }: { actions: readonly Action[]; hosts: Map<string, string> }) {
  return (
    <Stack gap={3}>
      {actions.map((action) => (
        <ActionCard key={action.id} action={action} host={hosts.get(action.host_id)} showIncident />
      ))}
    </Stack>
  );
}

/**
 * The actions waiting for a person, the soonest to expire first, and behind a closed
 * section those decided in the last day: on the selected host, or on all of them.
 */
export function ApprovalsPage() {
  const [selected] = useSelectedHost();
  // Fixed for the visit, so the list is one cached query and not a new one every render.
  const [since] = useState(() => new Date(Date.now() - RECENT_MS).toISOString());
  const waiting = useActions({ status: "proposed", host: selected });
  const decided = useActions({ decided: true, since, host: selected });
  const hosts = useHostNames();
  const every = useMemo(
    () => [...(waiting.data?.items ?? []), ...(decided.data?.items ?? [])],
    [waiting.data, decided.data],
  );
  useActionOutcomes(every, hosts);
  const count = waiting.data?.items.length ?? 0;

  return (
    <>
      <PageHeader
        title="Approvals"
        docs="concepts/actions.html"
        intro="Actions the model suggested. Nothing runs until you approve it; requests expire after an hour."
        meta={
          count > 0 && <Tag tone="pending">{count === 1 ? "1 waiting" : `${count} waiting`}</Tag>
        }
      />
      <Section title="Waiting for you">
        <QueryBoundary
          query={waiting}
          context="load approvals"
          loading={<Skeleton shape="block" />}
          isEmpty={(page) => page.items.length === 0}
          empty={
            <EmptyState icon="approve" title="Nothing to approve">
              <p>
                {selected
                  ? `No action is waiting on ${selected}. Choose All hosts to see the others.`
                  : "Actions triage suggests show up here, and nothing runs until you approve it."}
              </p>
            </EmptyState>
          }
        >
          {(page) => <Cards actions={byExpiry(page.items)} hosts={hosts} />}
        </QueryBoundary>
      </Section>
      <Section
        title="Recently decided"
        collapsed
        description="Approved, denied and expired in the last 24 hours, newest first."
      >
        <QueryBoundary
          query={decided}
          context="load recent decisions"
          loading={<Skeleton shape="block" />}
          isEmpty={(page) => page.items.length === 0}
          empty={
            <EmptyState title="Nothing was decided in the last 24 hours">
              <p>Actions you approve or deny, and ones that expire, are listed here for a day.</p>
            </EmptyState>
          }
        >
          {(page) => <Cards actions={page.items} hosts={hosts} />}
        </QueryBoundary>
      </Section>
    </>
  );
}
