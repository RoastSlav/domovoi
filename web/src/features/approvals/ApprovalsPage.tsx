import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import { EmptyState, LoadError, Loading, PageHeader } from "../../components";
import { ActionItem } from "./ActionItem";
import styles from "./approvals.module.css";

/** Every action waiting for a human, newest first. */
export function ApprovalsPage() {
  const actions = useActions("proposed");
  const hosts = useHostNames();
  const items = actions.data?.items ?? [];

  return (
    <>
      <PageHeader
        title="Approvals"
        docs="concepts/actions.html"
        intro="Actions the model suggested. Nothing runs until you approve it; requests expire after an hour."
      />
      {actions.isPending && <Loading />}
      {actions.error && !actions.data && (
        <LoadError error={actions.error} context="load approvals" onRetry={actions.refetch} />
      )}
      {actions.data && items.length === 0 && (
        <EmptyState icon="approve" title="Nothing to approve">
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
