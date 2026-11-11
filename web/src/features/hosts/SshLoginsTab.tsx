import { useState } from "react";
import { useIncidents } from "../../api/hooks/incidents";
import { useAckRules } from "../../api/hooks/rules";
import {
  Button,
  DocsLink,
  EmptyState,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
} from "../../components";
import { AddLoginDialog } from "./AddLoginDialog";
import { ExpectedLogins } from "./ExpectedLogins";
import { LoginWindow, LoginWindowNote } from "./LoginWindow";
import { SshFilters } from "./SshFilters";
import { SshLoginTable } from "./SshLoginTable";
import { ALL_LOGINS, filterLogins, loginsOf, type LoginEntry } from "./sshLogins";
import { SSH_CHECK, expectedOn, openWindow } from "./sshRules";

/** The history of logins with its filters; with none yet it says how they get here. */
function RecentLogins({ host }: { host: string }) {
  const incidents = useIncidents({ host, q: SSH_CHECK });
  const [filter, setFilter] = useState(ALL_LOGINS);
  const entries: LoginEntry[] | undefined = incidents.data
    ? loginsOf(incidents.data.pages.flatMap((page) => page.items))
    : undefined;
  return (
    <QueryBoundary
      query={{
        data: entries,
        error: incidents.error,
        isPending: incidents.isPending,
        refetch: incidents.refetch,
      }}
      context="load the SSH logins"
      loading={<Skeleton shape="rows" count={4} />}
      isEmpty={(all) => all.length === 0}
      empty={
        <EmptyState icon="user" title={`No SSH logins recorded on ${host}`}>
          <p>
            The agent reports every successful login it finds in the SSH journal. Once someone logs
            in, it appears here. If you expect one and nothing shows, check that the agent is
            connected.
          </p>
        </EmptyState>
      }
    >
      {(all) => (
        <Stack gap={3}>
          <SshFilters entries={all} filter={filter} onChange={setFilter} />
          <SshLoginTable all={all} host={host} shown={filterLogins(all, filter)} />
          {incidents.hasNextPage && (
            <div>
              <Button
                loading={incidents.isFetchingNextPage}
                onClick={() => void incidents.fetchNextPage()}
              >
                Show older logins
              </Button>
            </div>
          )}
        </Stack>
      )}
    </QueryBoundary>
  );
}

/** The expected logins and the actions on them: add one, or open a window for logging in now. */
function Expected({ host }: { host: string }) {
  const rules = useAckRules();
  const [adding, setAdding] = useState(false);
  const open = rules.data ? openWindow(rules.data, host, new Date().toISOString()) : undefined;
  return (
    <Section
      title="Expected logins"
      description={
        <>
          Every successful SSH login raises an incident. Logins you mark as expected are still
          recorded but never alert you.{" "}
          <DocsLink topic="expected SSH logins" docs="reference/checks.html#securityssh_login" />
        </>
      }
      actions={
        <>
          <Button icon="plus" onClick={() => setAdding(true)}>
            Add expected login
          </Button>
          <LoginWindow host={host} />
        </>
      }
    >
      <Stack gap={3}>
        {open && <LoginWindowNote host={host} open={open} />}
        <QueryBoundary
          query={rules}
          context="load the expected logins"
          loading={<Skeleton shape="rows" count={2} />}
        >
          {(all) => <ExpectedLogins host={host} rules={expectedOn(all, host)} />}
        </QueryBoundary>
      </Stack>
      {adding && <AddLoginDialog host={host} onClose={() => setAdding(false)} />}
    </Section>
  );
}

/**
 * SSH logins on one host: who logged in and from where, and the logins marked as expected,
 * which are recorded but never alert.
 */
export function SshLoginsTab({ host }: { host: string }) {
  return (
    <Stack gap={5}>
      <Section title="Recent logins">
        <RecentLogins host={host} />
      </Section>
      <Expected host={host} />
    </Stack>
  );
}
