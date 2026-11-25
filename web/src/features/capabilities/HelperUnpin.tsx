import { useState } from "react";
import { useUnpinHelperKey } from "../../api/hooks/helper";
import { Button, PasswordPrompt, useActionFeedback } from "../../components";

/**
 * Removes the pin after the password is asked again. Until a key is pinned again, the helper
 * on this host cannot be verified and containment is blocked.
 */
export function HelperUnpin({ host }: { host: string }) {
  const [open, setOpen] = useState(false);
  const unpin = useActionFeedback(useUnpinHelperKey(host), {
    success: `Removed the pin on ${host}. Containment there is blocked until you pin a key again.`,
    error: "remove the pin",
    errorDisplay: "inline",
  });
  const close = () => {
    setOpen(false);
    unpin.reset();
  };
  const failure = unpin.error && [unpin.error.title, unpin.error.detail].filter(Boolean).join(" ");
  return (
    <>
      <Button variant="secondary" icon="trash" onClick={() => setOpen(true)}>
        Unpin
      </Button>
      {open && (
        <PasswordPrompt
          title="Remove the pin"
          reason={`The helper on ${host} will read as unverified and containment there is blocked until you pin a key again.`}
          confirmLabel="Unpin"
          busy={unpin.isPending}
          error={failure || undefined}
          onCancel={close}
          onConfirm={(password) => unpin.mutate({ password }, { onSuccess: close })}
        />
      )}
    </>
  );
}
