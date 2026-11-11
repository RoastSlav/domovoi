// feedback-ok: useIncidentActions wraps ack, mute and unmute in useActionFeedback
import { useState } from "react";
import type { IncidentSummary } from "../../api/types";
import { Button, Menu } from "../../components";
import { DismissDialog } from "./DismissDialog";
import { MUTE_OPTIONS, useIncidentActions } from "./useIncidentActions";

export { MAX_REASON_CHARS } from "./DismissDialog";

/**
 * The incident's main actions, as far as its status allows: Acknowledge, Mute for a while
 * or Unmute, and Dismiss. Each button's hint says what it does and what it does not.
 */
export function IncidentControls({ incident }: { incident: IncidentSummary }) {
  const [dismissing, setDismissing] = useState(false);
  const { ack, mute, unmute } = useIncidentActions(incident.id);

  if (incident.status === "resolved") {
    return null;
  }
  return (
    <>
      {incident.status !== "acked" && (
        <Button
          variant="primary"
          hint="Stops reminders. The incident stays open and still resolves by itself."
          onClick={() => ack.mutate()}
          loading={ack.isPending}
        >
          Acknowledge
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
        <Menu
          text="Mute"
          label="Mute for"
          hint="No alerts or reminders for the time you pick. The incident stays open."
          loading={mute.isPending}
          items={MUTE_OPTIONS.map(([minutes, label]) => ({
            label: `For ${label}`,
            onSelect: () => mute.mutate(minutes),
          }))}
        />
      )}
      {/* A pending incident is not confirmed yet, and the hub refuses dismissing it, so the
          button would only offer a refusal. It appears once the incident is open. */}
      {incident.status !== "pending" && (
        <Button
          hint="Closes it as a known condition. You say why, and whether the check stays quiet."
          onClick={() => setDismissing(true)}
        >
          Dismiss
        </Button>
      )}
      {dismissing && <DismissDialog incident={incident} onClose={() => setDismissing(false)} />}
    </>
  );
}
