import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
import { usePublishDraft } from "../../api/hooks/policyCommands";
import { keys } from "../../api/queryClient";
import { ApiFailure } from "../../api/types";
import { Button, InlineMessage, PasswordPrompt, Stack, useActionFeedback } from "../../components";
import { stepUpError } from "../../design/messages";

/** What the controls need to publish exactly what the person read. */
export interface PublishControlsProps {
  /** Keys of the conflicts that must be acknowledged, as the hub listed them. */
  blocking: readonly string[];
  /** How many conflicts the person has acknowledged. */
  conflicts: number;
  /** Why publishing is not possible yet; undefined when it is. */
  disabledReason: string | undefined;
  /** How many changes to locked rules the review lists. */
  lockChanges: number;
  /** How many of the new version's rules are locked. */
  lockedCount: number;
  /** The version this publishes. */
  number: number;
  previewHash: string;
  /** How many rules the new version has. */
  ruleCount: number;
  revision: number;
}

/** What the person was shown when they chose Publish; this is what gets submitted. */
interface PublishAttempt {
  blocking: string[];
  conflicts: number;
  lockChanges: number;
  lockedCount: number;
  previewHash: string;
  revision: number;
  ruleCount: number;
}

/**
 * The Publish button and the password step behind it. Nothing is sent until the password is
 * confirmed; the password lives only in the dialog.
 * @param props What is being published, and why it cannot be yet if so.
 */
export function PublishControls(props: PublishControlsProps) {
  const { number, disabledReason } = props;
  const navigate = useNavigate();
  const client = useQueryClient();
  const [asking, setAsking] = useState<PublishAttempt>();
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const [stale, setStale] = useState(false);
  const publish = useActionFeedback(usePublishDraft(), {
    success: `Published policy version ${number}.`,
    error: "publish the policy",
    errorDisplay: "inline",
  });

  // A review that changed since the click is not what the person read; close the dialog.
  if (asking && (asking.previewHash !== props.previewHash || asking.revision !== props.revision)) {
    setAsking(undefined);
    setStale(false);
    setPasswordError(undefined);
  }

  const close = () => {
    setAsking(undefined);
    setStale(false);
    setPasswordError(undefined);
    publish.reset();
  };

  const confirm = (attempt: PublishAttempt, password: string) => {
    setPasswordError(undefined);
    setStale(false);
    publish.mutate(
      {
        revision: attempt.revision,
        previewHash: attempt.previewHash,
        acknowledged: attempt.blocking,
        password,
      },
      {
        onSuccess: () => void navigate("/policy"),
        onError: (error) => {
          setPasswordError(stepUpError(error));
          if (error instanceof ApiFailure && error.status === 409) {
            setStale(true);
            void client.invalidateQueries({ queryKey: keys.policy });
          }
        },
      },
    );
  };

  const failure = publish.error;
  const inline = failure && passwordError === undefined;
  return (
    <>
      <Button
        variant="primary"
        disabledReason={disabledReason}
        onClick={() =>
          setAsking({
            blocking: [...props.blocking].sort(),
            conflicts: props.conflicts,
            lockChanges: props.lockChanges,
            lockedCount: props.lockedCount,
            previewHash: props.previewHash,
            revision: props.revision,
            ruleCount: props.ruleCount,
          })
        }
      >
        {`Publish version ${number}`}
      </Button>
      {asking && (
        <PasswordPrompt
          title={`Publish version ${number}`}
          reason="Enter your password to publish this version."
          confirmLabel={`Publish version ${number}`}
          busy={publish.isPending}
          error={passwordError}
          onConfirm={(password) => confirm(asking, password)}
          onCancel={close}
        >
          <Stack gap={3}>
            <p>
              {`${asking.ruleCount} rules, ${asking.lockedCount} locked, ${asking.conflicts} conflicts acknowledged, ${asking.lockChanges} locked-rule changes.`}
            </p>
            {inline && (
              <InlineMessage
                tone="danger"
                title={failure.title}
                action={
                  stale && (
                    <Button size="sm" onClick={close}>
                      Review it again
                    </Button>
                  )
                }
              >
                {failure.detail}
                {failure.issues && (
                  <ul>
                    {failure.issues.map((issue) => (
                      <li key={`${issue.path}:${issue.message}`}>{issue.message}</li>
                    ))}
                  </ul>
                )}
              </InlineMessage>
            )}
          </Stack>
        </PasswordPrompt>
      )}
    </>
  );
}
