import { useState } from "react";
import { useAck, useDismiss, useMute, useUnmute } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import { Button, Dialog, ErrorDetail, InlineMessage, useActionFeedback } from "../../components";
import styles from "./incidents.module.css";

const MUTE_OPTIONS: [number, string][] = [
  [60, "1 hour"],
  [240, "4 hours"],
  [1440, "1 day"],
  [10080, "1 week"],
];

/** How a mute of `minutes` reads in a sentence: "1 hour". */
function muteLabel(minutes: number): string {
  return MUTE_OPTIONS.find(([value]) => value === minutes)?.[1] ?? `${minutes} minutes`;
}

/** How long a dismissal keeps the check quiet; `null` is "No", which creates no rule. */
const DISMISS_OPTIONS: [number | null, string][] = [
  [null, "No"],
  [1440, "1 day"],
  [10080, "1 week"],
  [43200, "30 days"],
  [0, "Always"],
];

/** The longest reason the hub takes, in characters. */
export const MAX_REASON_CHARS = 500;

/** The Dismiss dialog: why this is not a real problem, and how long to keep the check quiet. */
function DismissDialog({ incident, onClose }: { incident: IncidentSummary; onClose: () => void }) {
  const [reason, setReason] = useState("");
  const [quiet, setQuiet] = useState<number | null>(DISMISS_OPTIONS[1]?.[0] ?? 1440);
  const dismiss = useActionFeedback(useDismiss(incident.id), {
    success: "Incident dismissed as a known condition",
    error: "dismiss the incident",
    errorDisplay: "inline",
  });

  return (
    <Dialog
      title="Dismiss as a known condition"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={dismiss.isPending}
            disabledReason={reason.trim() === "" ? "Say why first." : undefined}
            onClick={() =>
              dismiss.mutate(
                { reason: reason.trim(), ignore_for_minutes: quiet },
                { onSuccess: onClose },
              )
            }
          >
            Dismiss
          </Button>
        </>
      }
    >
      <div className={styles.ask}>
        <label htmlFor="dismiss-reason">Why is this not a real problem?</label>
        <textarea
          id="dismiss-reason"
          aria-describedby="dismiss-hint"
          value={reason}
          maxLength={MAX_REASON_CHARS}
          onChange={(e) => setReason(e.target.value)}
          placeholder="For example: the nightly backup fills it, and it clears afterwards."
        />
        <div className={styles.filter}>
          <label htmlFor="dismiss-quiet">Keep the check quiet for</label>
          <select
            id="dismiss-quiet"
            value={String(quiet)}
            onChange={(e) => setQuiet(e.target.value === "null" ? null : Number(e.target.value))}
          >
            {DISMISS_OPTIONS.map(([value, label]) => (
              <option key={String(value)} value={String(value)}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <p className={styles.hint} id="dismiss-hint">
          The incident closes as an expected condition, so it stops alerting and no triage runs. If
          the check keeps failing, the incident reopens quietly: no alert and no triage. The rule
          covers every severity of this check on this target, so a critical later on the same mount
          is quiet too.
        </p>
        {dismiss.error && (
          <InlineMessage tone="danger" title={dismiss.error.title}>
            <ErrorDetail error={dismiss.error} />
          </InlineMessage>
        )}
      </div>
    </Dialog>
  );
}

/** Ack, Mute, Unmute and Dismiss, as far as the incident's status allows. */
export function IncidentControls({ incident }: { incident: IncidentSummary }) {
  const [minutes, setMinutes] = useState(MUTE_OPTIONS[0]?.[0] ?? 60);
  const [dismissing, setDismissing] = useState(false);
  const ack = useActionFeedback(useAck(incident.id), {
    success: "Incident acknowledged",
    error: "acknowledge the incident",
  });
  const unmute = useActionFeedback(useUnmute(incident.id), {
    success: "Incident unmuted",
    error: "unmute the incident",
  });
  const mute = useActionFeedback(useMute(incident.id), {
    success: (_, muted) => `Incident muted for ${muteLabel(muted)}`,
    error: "mute the incident",
    undo: () => unmute.mutate(),
  });

  if (incident.status === "resolved") {
    return null;
  }
  return (
    <div className={styles.toolbar}>
      {incident.status !== "acked" && (
        <Button
          hint="Stops reminders; the incident stays listed until the problem clears."
          onClick={() => ack.mutate()}
          loading={ack.isPending}
        >
          Ack
        </Button>
      )}
      {incident.status === "muted" ? (
        <Button
          hint="Lifts the mute, so alerts and reminders start again."
          onClick={() => unmute.mutate()}
          loading={unmute.isPending}
        >
          Unmute
        </Button>
      ) : (
        <>
          <div className={styles.filter}>
            <label htmlFor="mute-for">Mute for</label>
            <select
              id="mute-for"
              value={minutes}
              onChange={(e) => setMinutes(Number(e.target.value))}
            >
              {MUTE_OPTIONS.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
          <Button
            hint="No alerts or reminders until the time you pick."
            onClick={() => mute.mutate(minutes)}
            loading={mute.isPending}
          >
            Mute
          </Button>
        </>
      )}
      {/* A pending incident is not confirmed yet, and core refuses dismissing it, so the
          button would only offer a refusal. It appears once the incident is open. */}
      {incident.status !== "pending" && (
        <Button
          hint="Closes the incident as a known condition, so it stops alerting."
          onClick={() => setDismissing(true)}
        >
          Dismiss
        </Button>
      )}
      {dismissing && <DismissDialog incident={incident} onClose={() => setDismissing(false)} />}
    </div>
  );
}
