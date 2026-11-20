import { useMemo, useState } from "react";
import { Link } from "react-router";
import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import { useSelectedHost } from "../../app/HostContext";
import { EmptyState, PageHeader, QueryBoundary, Section, Skeleton } from "../../components";
import { DecidedList } from "./DecidedList";
import { useActionOutcomes } from "./useActionOutcomes";

/** How far back "Recently decided" looks. */
const RECENT_MS = 24 * 60 * 60 * 1000;

/**
 * Where approvals happen, and behind a closed section the legacy actions that ended in the
 * last day, read-only: on the selected host, or on all of them. Approving is done on the
 * operation that asks, so nothing here decides.
 */
export function ApprovalsPage() {
  const [selected] = useSelectedHost();
  // Fixed for the visit, so the list is one cached query and not a new one every render.
  const [since] = useState(() => new Date(Date.now() - RECENT_MS).toISOString());
  const decided = useActions({ decided: true, since, host: selected });
  const hosts = useHostNames();
  const every = useMemo(() => decided.data?.items ?? [], [decided.data]);
  useActionOutcomes(every, hosts);

  return (
    <>
      <PageHeader
        title="Approvals"
        docs="concepts/actions.html"
        intro="Nothing runs until you approve it. Approvals are decided on the operation that asks."
      />
      <Section title="Waiting for you">
        <EmptyState icon="approve" title="Approve on the operation">
          <p>
            Each step that needs you waits on its operation, with the exact plan to approve. Open
            the <Link to="/operations">operations</Link> to see what is waiting.
          </p>
        </EmptyState>
      </Section>
      <Section title="Recently decided" collapsed description="The last 24 hours, newest first.">
        <QueryBoundary
          query={decided}
          context="load recent decisions"
          loading={<Skeleton shape="block" />}
          isEmpty={(page) => page.items.length === 0}
          empty={
            <EmptyState title="Nothing ended in the last 24 hours">
              <p>
                Legacy actions that ended, including ones that expired, are listed here for a day.
              </p>
            </EmptyState>
          }
        >
          {(page) => <DecidedList actions={page.items} hosts={hosts} />}
        </QueryBoundary>
      </Section>
    </>
  );
}
