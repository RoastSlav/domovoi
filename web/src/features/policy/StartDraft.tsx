import { useNavigate } from "react-router";
import { useStartDraft } from "../../api/hooks/policyCommands";
import { Button, Cluster, useActionFeedback, type ButtonVariant } from "../../components";

/**
 * Copies a published version's rules (or nothing) into a new draft and opens the editor.
 * It carries no visibility category: the page that shows it decides how it is grouped.
 * @param from The version to copy, or null to start empty.
 * @param label The button text.
 * @param variant How prominent the button is.
 * @param hint Says what the button does when its label alone does not.
 * @param disabledReason Why the button cannot be used now.
 */
export function StartDraftButton({
  from,
  label,
  variant = "secondary",
  hint,
  disabledReason,
}: {
  disabledReason?: string | undefined;
  from: number | null;
  hint?: string;
  label: string;
  variant?: ButtonVariant;
}) {
  const navigate = useNavigate();
  const start = useActionFeedback(useStartDraft(), {
    success: "Draft started.",
    error: "start the draft",
  });
  return (
    <Button
      variant={variant}
      hint={hint}
      disabledReason={disabledReason}
      loading={start.isPending}
      onClick={() => start.mutate({ from }, { onSuccess: () => void navigate("/policy/draft") })}
    >
      {label}
    </Button>
  );
}

/**
 * The ways to begin a draft when there is none: from the active version, or from nothing.
 * @param from The active version, or null when none is published.
 */
export function StartDraft({ from }: { from: number | null }) {
  if (from === null) {
    return <StartDraftButton variant="primary" from={null} label="Start a draft" />;
  }
  return (
    <Cluster gap={2}>
      <StartDraftButton
        variant="primary"
        from={from}
        label={`Start from version ${from}`}
        hint="Copies the active version's rules into an editable draft."
      />
      <StartDraftButton from={null} label="Start empty" hint="Begins a draft with no rules." />
    </Cluster>
  );
}
