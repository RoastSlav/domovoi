import { useState } from "react";
import type { Action, Report } from "../../api/types";
import { Button, RelativeTime, Section, Spinner, StatusPill, Text } from "../../components";
import { actionStatusMeta } from "../../design/tone";
import { ActionCard } from "../approvals/ActionCard";
import { whyProposed } from "../approvals/whyProposed";
import styles from "./incidents.module.css";

/** How many finished actions show before "Show all". */
export const EARLIER_PREVIEW = 4;

const IN_PROGRESS = new Set(["approved", "dispatched"]);

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
  const failed = ["failed", "verify_failed", "rejected"].includes(action.status);
  return (
    <li className={styles.item}>
      <div className={styles.actionRow}>
        <span className={styles.actionLine}>
          <span className={styles.skill}>{action.skill}</span>
        </span>
        <StatusPill size="sm" meta={actionStatusMeta(action.status)} />
        <span className={styles.rowWhen}>
          {running ? <Spinner size={14} /> : <RelativeTime at={action.updated_at} />}
        </span>
      </div>
      {Object.keys(action.args).length > 0 && <p className={styles.argsLine}>{argsLine(action)}</p>}
      {running && <p className={styles.rowNote}>{runningNote(action, host)}</p>}
      {failed && action.reason && <p className={styles.rowFailure}>{action.reason}</p>}
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

/** The soonest to expire first. */
function byExpiry(actions: readonly Action[]): Action[] {
  return [...actions].sort((a, b) => Date.parse(a.expires_at) - Date.parse(b.expires_at));
}

/**
 * The incident's actions by what the person can do about them: the ones waiting for a
 * decision as full cards (soonest to expire first), the ones running as compact rows, and
 * every finished one as a one-line row under "Earlier actions", four at a time. Each group
 * is its own slot of the page, so the cards that need a decision are first.
 */
export function ActionSections({
  actions,
  host,
  report,
  ackIsPrimary,
}: {
  actions: readonly Action[];
  /** Acknowledge is the page's primary button, so Approve steps back to secondary. */
  ackIsPrimary: boolean;
  host: string;
  report: Report | null | undefined;
}) {
  const [all, setAll] = useState(false);
  const waiting = byExpiry(actions.filter((action) => action.status === "proposed"));
  const running = actions.filter((action) => IN_PROGRESS.has(action.status));
  const earlier = actions.filter(
    (action) => action.status !== "proposed" && !IN_PROGRESS.has(action.status),
  );
  const shown = all ? earlier : earlier.slice(0, EARLIER_PREVIEW);

  if (actions.length === 0) {
    return (
      <div className={styles.slot} data-slot="waiting">
        <Section title="Actions">
          <Text block muted size="sm">
            No actions proposed. Nothing runs on a host unless you approve it here.
          </Text>
        </Section>
      </div>
    );
  }
  return (
    <>
      {waiting.length > 0 && (
        <div className={styles.slot} data-slot="waiting">
          <Section title={`Waiting for you · ${waiting.length}`}>
            <div className={styles.actionCards}>
              {waiting.map((action) => (
                <ActionCard
                  key={action.id}
                  action={action}
                  host={host}
                  approve={ackIsPrimary ? "secondary" : "primary"}
                  why={whyProposed(action, report)}
                />
              ))}
            </div>
          </Section>
        </div>
      )}
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
