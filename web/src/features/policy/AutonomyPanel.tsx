import { useState } from "react";
import { useAutonomy, usePauseAutonomy, useResumeAutonomy } from "../../api/hooks/policyAutonomy";
import type { Autonomy } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  Field,
  InlineMessage,
  PasswordPrompt,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  Text,
  TextInput,
  useActionFeedback,
} from "../../components";
import { stepUpError } from "../../design/messages";
import { SuspendedRules } from "./SuspendedRules";

const REASON_LIMIT = 280;

function PauseDialog({ onClose }: { onClose: () => void }) {
  const [reason, setReason] = useState("");
  const pause = useActionFeedback(usePauseAutonomy(), {
    success: "Autonomy paused. Nothing runs without you.",
    error: "pause autonomy",
    errorDisplay: "inline",
  });
  const trimmed = reason.trim();
  return (
    <ConfirmDialog
      title="Pause autonomy?"
      consequence="No new permit is issued and no permit already issued is used. Restarts wait for you to approve them."
      confirmLabel="Pause"
      tone="danger"
      confirmDisabledReason={trimmed ? undefined : "Say why first."}
      busy={pause.isPending}
      error={pause.error}
      onCancel={onClose}
      onConfirm={() => pause.mutate({ reason: trimmed }, { onSuccess: onClose })}
    >
      <Field label="Reason" hint={`Up to ${REASON_LIMIT} characters. It is kept in the audit log.`}>
        <TextInput
          value={reason}
          maxLength={REASON_LIMIT}
          onChange={(event) => setReason(event.target.value)}
        />
      </Field>
    </ConfirmDialog>
  );
}

function ResumeDialog({ onClose }: { onClose: () => void }) {
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const resume = useActionFeedback(useResumeAutonomy(), {
    success: "Autonomy resumed.",
    error: "resume autonomy",
    errorDisplay: "inline",
  });
  return (
    <PasswordPrompt
      title="Resume autonomy"
      reason="Enter your password to let rules that allow it restart services without asking."
      confirmLabel="Resume"
      busy={resume.isPending}
      error={passwordError ?? resume.error?.title}
      onCancel={onClose}
      onConfirm={(password) =>
        resume.mutate(
          { password },
          { onSuccess: onClose, onError: (error) => setPasswordError(stepUpError(error)) },
        )
      }
    />
  );
}

function State({ autonomy }: { autonomy: Autonomy }) {
  const [dialog, setDialog] = useState<"pause" | "resume" | null>(null);
  const close = () => setDialog(null);
  return (
    <Stack gap={3}>
      {autonomy.paused ? (
        <InlineMessage tone="warning" title="Autonomy is paused">
          {autonomy.reason}
          {autonomy.paused_at && (
            <>
              {" "}
              (<RelativeTime at={autonomy.paused_at} />
              {autonomy.paused_by ? `, by ${autonomy.paused_by}` : ""})
            </>
          )}
        </InlineMessage>
      ) : (
        <Text muted>
          Rules that allow it may restart services without asking, within their limits.
        </Text>
      )}
      <div>
        {autonomy.paused ? (
          <Button variant="primary" onClick={() => setDialog("resume")}>
            Resume
          </Button>
        ) : (
          <Button variant="danger" onClick={() => setDialog("pause")}>
            Pause
          </Button>
        )}
      </div>
      <SuspendedRules rules={autonomy.suspended_rules} />
      {dialog === "pause" && <PauseDialog onClose={close} />}
      {dialog === "resume" && <ResumeDialog onClose={close} />}
    </Stack>
  );
}

/**
 * The switch that stops unattended action, and the rules whose breaker is open. Pausing is one
 * click and needs no password; resuming and resetting a rule do.
 */
export function AutonomyPanel() {
  const autonomy = useAutonomy();
  return (
    <QueryBoundary
      query={autonomy}
      context="load the autonomy state"
      loading={<Skeleton shape="rows" count={2} />}
    >
      {(data) => <State autonomy={data} />}
    </QueryBoundary>
  );
}
