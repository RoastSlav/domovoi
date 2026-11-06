import { useState } from "react";
import { useAckRules, useDeleteAckRule } from "../../api/hooks/rules";
import type { AckRule, Predicate } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import { RelativeTime } from "../../components/RelativeTime";
import ui from "../../components/ui.module.css";
import { AddRuleForm, opLabel } from "./AddRuleForm";
import styles from "./rules.module.css";

/** A rule's expiry as the table shows it. */
function Expiry({ at }: { at: string | null }) {
  if (at === null) {
    return <span className={ui.muted}>never</span>;
  }
  const date = new Date(at);
  const time = date.getTime();
  if (Number.isNaN(time)) {
    return <span>{at}</span>;
  }
  if (time <= new Date().getTime()) {
    return <span className={styles.expired}>expired</span>;
  }
  return <RelativeTime at={at} />;
}

/** The conditions of a rule, in words, all of which must hold. */
function conditionsText(conditions: Predicate[]): string {
  if (conditions.length === 0) {
    return "every signal";
  }
  return conditions.map((c) => `${c.key} ${opLabel(c.op)} ${c.value}`).join("; ");
}

/** Delete, asking first. */
function DeleteButton({ rule }: { rule: AckRule }) {
  const [open, setOpen] = useState(false);
  const remove = useDeleteAckRule();
  const close = () => {
    setOpen(false);
    remove.reset();
  };
  return (
    <>
      <Button
        variant="danger"
        aria-label={`Delete the known condition ${rule.check}`}
        onClick={() => setOpen(true)}
      >
        Delete
      </Button>
      {open && (
        <ConfirmDialog
          title={`Delete the known condition on ${rule.host ?? "any host"}?`}
          confirmLabel="Delete"
          confirmVariant="danger"
          busy={remove.isPending}
          onConfirm={() => remove.mutate(rule.id, { onSuccess: close })}
          onCancel={close}
        >
          <p>“{rule.note}”</p>
          <p>
            Incidents matching <code>{rule.check}</code> on {rule.host ?? "any host"} start
            notifying again. Ones that already matched stay in the history.
          </p>
          {remove.error && (
            <p className={styles.error} role="alert">
              Could not delete the known condition. Try again.
            </p>
          )}
        </ConfirmDialog>
      )}
    </>
  );
}

/** Known conditions: add, list and delete them. */
export function RulesPage() {
  const rules = useAckRules();
  const items = rules.data ?? [];

  return (
    <>
      <div className={ui.pageHead}>
        <h1>Known conditions</h1>
      </div>
      <PageIntro topic="known conditions" docs="concepts/incidents.html#known-conditions">
        Incidents that match a known condition are kept in the history but never alert you and are
        not triaged.
      </PageIntro>
      {rules.isPending && <p>Loading…</p>}
      {rules.error && <EmptyState title="Could not load the known conditions" />}
      {rules.data && items.length === 0 && (
        <EmptyState title="No known conditions">
          <p>Add one below: matching incidents stay in the history and never alert you.</p>
        </EmptyState>
      )}
      {items.length > 0 && (
        <div className={ui.tableWrap}>
          <table className={ui.table}>
            <thead>
              <tr>
                <th scope="col">Host</th>
                <th scope="col">Check</th>
                <th scope="col">Target</th>
                <th scope="col">Conditions</th>
                <th scope="col">Note</th>
                <th scope="col">Expires</th>
                <th scope="col">
                  <span className="visually-hidden">Delete</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((rule) => (
                <tr key={rule.id}>
                  <td>{rule.host ?? "any host"}</td>
                  <td>
                    <code>{rule.check}</code>
                  </td>
                  <td>{rule.target_pattern ?? "any target"}</td>
                  <td>{conditionsText(rule.predicates)}</td>
                  <td>{rule.note}</td>
                  <td>
                    <Expiry at={rule.expires_at ?? null} />
                  </td>
                  <td>
                    <DeleteButton rule={rule} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <AddRuleForm />
    </>
  );
}
