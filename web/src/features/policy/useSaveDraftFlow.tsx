import { useState, type ReactNode } from "react";
import { usePreviewRules, useSaveDraft } from "../../api/hooks/policyCommands";
import {
  ApiFailure,
  type PolicyDraft,
  type PolicyDraftSaved,
  type PolicyPreview,
  type PolicyRule,
} from "../../api/types";
import { useActionFeedback } from "../../components";
import { stepUpError } from "../../design/messages";
import { LockedChangeDialog } from "./LockedChangeDialog";
import type { RuleIssue } from "./ruleIssues";

/** What a review covered: the save must submit exactly this, whatever the page does meanwhile. */
interface SaveAttempt {
  baseRevision: number;
  preview: PolicyPreview;
  rules: PolicyRule[];
}

function issuesOf(failure: unknown): RuleIssue[] {
  return failure instanceof ApiFailure ? failure.issues : [];
}

function savedText(locked: number): string {
  if (locked === 0) return "Draft saved.";
  return locked === 1
    ? "Draft saved. 1 locked rule change was recorded."
    : `Draft saved. ${locked} locked rule changes were recorded.`;
}

/**
 * Saving the working copy: review it first, save at once when no locked rule changes, and
 * otherwise show the changes and ask for the password in a dialog. The password lives only in
 * the dialog and goes to the hub with the review's hash.
 * @param saved The draft as last stored; its revision is what a save is based on.
 * @param working The rules to save.
 * @param onSaved Receives the stored draft.
 * @returns `onSave` to start, `dialog` to render, the hub's `issues` about the rules and `saving`.
 */
export function useSaveDraftFlow({
  saved,
  working,
  onSaved,
}: {
  onSaved: (draft: PolicyDraft) => void;
  saved: PolicyDraft;
  working: PolicyRule[];
}): {
  clearIssues: () => void;
  dialog: ReactNode;
  issues: RuleIssue[];
  onSave: () => void;
  saving: boolean;
} {
  const [issues, setIssues] = useState<RuleIssue[]>([]);
  const [asking, setAsking] = useState<SaveAttempt>();
  const [passwordError, setPasswordError] = useState<string>();
  const [conflict, setConflict] = useState(false);
  const save = useSaveDraft();
  const check = useActionFeedback(usePreviewRules(), {
    success: () => null,
    error: "check the rules",
  });
  const message = {
    success: (data: PolicyDraftSaved) => savedText(data.locked_changes.length),
  };
  const plain = useActionFeedback(save, { ...message, error: "save the draft" });
  const locked = useActionFeedback(save, {
    ...message,
    error: "save the draft",
    errorDisplay: "inline",
  });
  const request = { baseRevision: saved.revision, rules: working };

  // A review of other rules or of another revision is no longer what would be saved.
  if (asking && (asking.rules !== working || asking.baseRevision !== saved.revision)) {
    setAsking(undefined);
    setPasswordError(undefined);
  }

  const onSave = () => {
    setIssues([]);
    setPasswordError(undefined);
    setConflict(false);
    locked.reset();
    check.mutate(request, {
      onError: (failure) => setIssues(issuesOf(failure)),
      onSuccess: (preview) => {
        if (preview.locked_changes.length > 0) {
          setAsking({ ...request, preview });
          return;
        }
        plain.mutate(request, {
          onSuccess: (result) => onSaved(result.draft),
          onError: (failure) => setIssues(issuesOf(failure)),
        });
      },
    });
  };

  const confirm = (attempt: SaveAttempt, password: string) => {
    setPasswordError(undefined);
    locked.mutate(
      {
        baseRevision: attempt.baseRevision,
        rules: attempt.rules,
        password,
        previewHash: attempt.preview.preview_hash,
      },
      {
        onSuccess: (result) => {
          setAsking(undefined);
          onSaved(result.draft);
        },
        onError: (failure) => {
          const stepUp = stepUpError(failure);
          setPasswordError(stepUp);
          setConflict(failure instanceof ApiFailure && failure.status === 409);
          const found = issuesOf(failure);
          if (stepUp === undefined && failure instanceof ApiFailure && failure.status === 400) {
            // Without rule issues there is nothing for the banner to say; the dialog explains.
            if (found.length > 0) {
              setAsking(undefined);
              setIssues(found);
            }
          }
        },
      },
    );
  };

  const close = () => {
    setAsking(undefined);
    setPasswordError(undefined);
    locked.reset();
  };

  const dialog = asking ? (
    <LockedChangeDialog
      preview={asking.preview}
      busy={locked.isPending}
      passwordError={passwordError}
      problem={passwordError ? undefined : locked.error}
      onConfirm={(password) => confirm(asking, password)}
      onCancel={close}
      canCheckAgain={conflict}
      onCheckAgain={() => {
        close();
        onSave();
      }}
    />
  ) : null;

  return {
    clearIssues: () => setIssues([]),
    dialog,
    issues,
    onSave,
    saving: check.isPending || save.isPending,
  };
}
