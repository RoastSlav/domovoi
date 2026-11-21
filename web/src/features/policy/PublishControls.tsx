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

/**
 * The Publish button and the password step behind it. Nothing is sent until the password is
 * confirmed; the password lives only in the dialog.
 * @param props What is being published, and why it cannot be yet if so.
 */
export function PublishControls(props: PublishControlsProps) {
  const { number, disabledReason } = props;
  const navigate = useNavigate();
  const client = useQueryClient();
  const [asking, setAsking] = useState(false);
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const [stale, setStale] = useState(false);
  const publish = useActionFeedback(usePublishDraft(), {
    success: `Published policy version ${number}.`,
    error: "publish the policy",
    errorDisplay: "inline",
  });

  const close = () => {
    setAsking(false);
    setStale(false);
    setPasswordError(undefined);
    publish.reset();
  };

  const confirm = (password: string) => {
    setPasswordError(undefined);
    setStale(false);
    publish.mutate(
      {
        revision: props.revision,
        previewHash: props.previewHash,
        acknowledged: [...props.blocking].sort(),
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
      <Button variant="primary" disabledReason={disabledReason} onClick={() => setAsking(true)}>
        {`Publish version ${number}`}
      </Button>
      {asking && (
        <PasswordPrompt
          title={`Publish version ${number}`}
          reason="Enter your password to publish this version."
          confirmLabel={`Publish version ${number}`}
          busy={publish.isPending}
          error={passwordError}
          onConfirm={confirm}
          onCancel={close}
        >
          <Stack gap={3}>
            <p>
              {`${props.ruleCount} rules, ${props.lockedCount} locked, ${props.conflicts} conflicts acknowledged, ${props.lockChanges} locked-rule changes.`}
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
