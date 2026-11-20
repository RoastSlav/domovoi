import { useState } from "react";
import type { Action } from "../../api/types";
import { Button, RelativeTime, Section, Spinner, StatusPill, Tag, Text } from "../../components";
import { actionStatusMeta } from "../../design/tone";
import styles from "./incidents.module.css";

/** How many finished actions show before "Show all". */
export const EARLIER_PREVIEW = 4;

const IN_PROGRESS = new Set(["approved", "dispatched"]);

/** Finished statuses with a reason worth a line of its own. */
const SHOWS_REASON = [
  "failed",
  "verify_failed",
  "rejected",
  "outcome_unknown",
  "not_run",
  "expired",
];

/** The reason's tone: a warning needs checking, a neutral one only says what happened. */
function reasonClass(status: string): string | undefined {
  if (status === "not_run" || status === "expired") {
    return styles.rowNote;
  }
  return status === "outcome_unknown" ? styles.rowWarning : styles.rowFailure;
}

/** The arguments in full: `name = web  ·  path = /srv`. */
function argsLine(action: Action): string {
  return Object.entries(action.args)
    .map(([name, value]) => `${name} = ${value}`)
    .join("  ·  ");
}

/** What a running action is doing, in a sentence. */
function runningNote(action: Action, host: string): string {
  return action.status === "approved"
    ? "Approved. Sending it to the agent…"
    : `Running on ${host}. Waiting for the result…`;
}

/** One action as one compact row: skill and arguments on a line, status, and when or a spinner. */
function ActionRow({ action, host }: { action: Action; host: string }) {
  const running = IN_PROGRESS.has(action.status);
  const showsReason = SHOWS_REASON.includes(action.status);
  return (
    <li className={styles.item}>
      <div className={styles.actionRow}>
        <span className={styles.actionLine}>
          <span className={styles.skill}>{action.skill}</span>
          <Tag>Legacy</Tag>
        </span>
        <StatusPill size="sm" meta={actionStatusMeta(action.status)} />
        <span className={styles.rowWhen}>
          {running ? <Spinner size={14} /> : <RelativeTime at={action.updated_at} />}
        </span>
      </div>
      {Object.keys(action.args).length > 0 && <p className={styles.argsLine}>{argsLine(action)}</p>}
      {running && <p className={styles.rowNote}>{runningNote(action, host)}</p>}
      {showsReason && action.reason && (
        <p className={reasonClass(action.status)}>{action.reason}</p>
      )}
    </li>
  );
}

function Rows({ actions, host }: { actions: readonly Action[]; host: string }) {
  return (
    <ul className={styles.list}>
      {actions.map((action) => (
        <ActionRow key={action.id} action={action} host={host} />
      ))}
    </ul>
  );
}

/**
 * The incident's actions, all read-only history of the older flow: the ones still running
 * as compact rows, and every finished one as a one-line row under "Earlier actions", four at
 * a time. Approving is done on the operations the incident links to.
 */
export function ActionSections({ actions, host }: { actions: readonly Action[]; host: string }) {
  const [all, setAll] = useState(false);
  const running = actions.filter((action) => IN_PROGRESS.has(action.status));
  const earlier = actions.filter((action) => !IN_PROGRESS.has(action.status));
  const shown = all ? earlier : earlier.slice(0, EARLIER_PREVIEW);

  if (actions.length === 0) {
    return (
      <div className={styles.slot} data-slot="waiting">
        <Section title="Actions">
          <Text block muted size="sm">
            No earlier actions. Steps that need you wait on the incident's operations.
          </Text>
        </Section>
      </div>
    );
  }
  return (
    <>
      {running.length > 0 && (
        <div className={styles.slot} data-slot="progress">
          <Section title={`In progress · ${running.length}`}>
            <Rows actions={running} host={host} />
          </Section>
        </div>
      )}
      {earlier.length > 0 && (
        <div className={styles.slot} data-slot="earlier">
          <Section title={`Earlier actions · ${earlier.length}`}>
            <Rows actions={shown} host={host} />
            {earlier.length > EARLIER_PREVIEW && (
              <Button size="sm" variant="ghost" onClick={() => setAll((now) => !now)}>
                {all ? "Show fewer" : `Show all ${earlier.length}`}
              </Button>
            )}
          </Section>
        </div>
      )}
    </>
  );
}
