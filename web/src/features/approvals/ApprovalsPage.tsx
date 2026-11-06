import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import { ActionItem } from "./ActionItem";
import styles from "./approvals.module.css";

/** Every action waiting for a human, newest first. */
export function ApprovalsPage() {
  const actions = useActions("proposed");
  const hosts = useHostNames();
  const items = actions.data?.items ?? [];

  return (
    <>
      <h1>Approvals</h1>
      <PageIntro docs="concepts/actions.html">
        Actions the model suggested. Nothing runs until you approve it; requests expire after an
        hour.
      </PageIntro>
      {actions.isPending && <p>Loading…</p>}
      {actions.error && <EmptyState title="Could not load approvals" />}
      {actions.data && items.length === 0 && (
        <EmptyState title="Nothing to approve">
          <p>Actions triage proposes show up here.</p>
        </EmptyState>
      )}
      {items.length > 0 && (
        <ul className={styles.list}>
          {items.map((action) => (
            <ActionItem
              key={action.id}
              action={action}
              host={hosts.get(action.host_id)}
              showIncident
            />
          ))}
        </ul>
      )}
    </>
  );
}
