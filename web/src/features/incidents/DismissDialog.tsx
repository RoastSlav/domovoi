import { useState } from "react";
import { useDismiss } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import {
  Button,
  Dialog,
  ErrorDetail,
  Field,
  InlineMessage,
  Select,
  Stack,
  Textarea,
  useActionFeedback,
} from "../../components";

/** The longest reason the hub takes, in characters. */
export const MAX_REASON_CHARS = 500;

/** From this many characters on, the reason field counts them. */
const COUNT_FROM = MAX_REASON_CHARS - 100;

const KNOWN =
  "Creates a known condition: matching incidents are kept in the history and never notify";

/** How long a dismissal keeps the check quiet, and what each choice does. `null` creates no rule. */
const QUIET: { consequence: string; label: string; minutes: number | null }[] = [
  {
    minutes: null,
    label: "No",
    consequence:
      "Closes this incident now and creates no known condition. The check alerts again the next time it fires.",
  },
  { minutes: 1440, label: "1 day", consequence: `${KNOWN}, for 1 day.` },
  { minutes: 10080, label: "1 week", consequence: `${KNOWN}, for 1 week.` },
  { minutes: 43200, label: "30 days", consequence: `${KNOWN}, for 30 days.` },
  { minutes: 0, label: "Always", consequence: `${KNOWN}, until you remove it.` },
];

/**
 * Dismisses an incident as a known condition: why it is not a real problem, and how long
 * the check stays quiet, with what that choice does spelled out.
 */
export function DismissDialog({
  incident,
  onClose,
}: {
  incident: IncidentSummary;
  onClose: () => void;
}) {
  const [reason, setReason] = useState("");
  const [choice, setChoice] = useState(1);
  const quiet = QUIET[choice] ?? QUIET[0];
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
                { reason: reason.trim(), ignore_for_minutes: quiet?.minutes ?? null },
                { onSuccess: onClose },
              )
            }
          >
            Dismiss
          </Button>
        </>
      }
    >
      <Stack gap={3}>
        <Field
          label="Why is this not a real problem?"
          required
          hint={
            reason.length >= COUNT_FROM
              ? `${reason.length} of ${MAX_REASON_CHARS} characters`
              : "For example: the nightly backup fills it, and it clears afterwards."
          }
        >
          <Textarea
            value={reason}
            maxLength={MAX_REASON_CHARS}
            onChange={(event) => setReason(event.target.value)}
          />
        </Field>
        <Field
          label="Keep the check quiet for"
          hint="The incident closes as expected. If the check keeps failing while it is quiet, it reopens without an alert or triage. A known condition covers every severity of this check on this target."
        >
          <Select value={choice} onChange={(event) => setChoice(Number(event.target.value))}>
            {QUIET.map((option, index) => (
              <option key={option.label} value={index}>
                {option.label}
              </option>
            ))}
          </Select>
        </Field>
        <InlineMessage tone="info">{quiet?.consequence}</InlineMessage>
        {dismiss.error && (
          <InlineMessage tone="danger" title={dismiss.error.title}>
            <ErrorDetail error={dismiss.error} />
          </InlineMessage>
        )}
      </Stack>
    </Dialog>
  );
}
