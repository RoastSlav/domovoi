import { useState } from "react";
import { useSetPackageSkills } from "../../api/hooks/hostPackages";
import { ConfirmDialog, Stack, Text, useActionFeedback } from "../../components";
import { PasswordField } from "./PasswordField";
import { useLockout } from "./useLockout";

/**
 * A person's decision to let a stored package's skills run on one host, or to stop them. It
 * asks for the password and the typed package name, like an install. The agent checks the
 * package files again and may refuse, in which case its words are shown here.
 */
export function PackageSkillsDialog({
  host,
  hostId,
  name,
  version,
  enable,
  onClose,
}: {
  enable: boolean;
  host: string;
  hostId: string;
  name: string;
  onClose: () => void;
  version: string;
}) {
  const [password, setPassword] = useState("");
  const [typed, setTyped] = useState("");
  const lockout = useLockout();
  const mutation = useSetPackageSkills(hostId, name);
  const verb = enable ? "Enable" : "Disable";
  const change = useActionFeedback(mutation, {
    success: `${enable ? "Enabled" : "Disabled"} the skills of ${name} on ${host}`,
    error: `${verb.toLowerCase()} the skills`,
    errorDisplay: "inline",
  });
  const reason = password === "" ? "Enter your password first." : lockout.reason;
  return (
    <ConfirmDialog
      title={`${verb} the skills of ${name} ${version} on ${host}?`}
      consequence={
        <Stack gap={2}>
          <Text>
            {enable
              ? `The agent on ${host} checks that the package files are exactly what was installed and signed, then lets its skills run. Each skill still follows the read, write and destructive rules of every other skill, and nothing runs without the usual approval.`
              : `The skills of ${name} stop running on ${host}. The files stay.`}
          </Text>
          <Text muted size="sm">
            The decision is recorded in the audit log with your name and the plan hash.
          </Text>
        </Stack>
      }
      confirmLabel={verb}
      requireText={name}
      onTyped={setTyped}
      confirmDisabledReason={reason}
      busy={change.isPending}
      error={change.error}
      onConfirm={() =>
        change.mutate(
          { enabled: enable, version, password, confirmText: typed },
          {
            onSuccess: () => {
              mutation.reset();
              setPassword("");
              onClose();
            },
            onError: (error) => {
              mutation.reset();
              setPassword("");
              lockout.lock(error);
            },
          },
        )
      }
      onCancel={onClose}
    >
      <PasswordField value={password} onChange={setPassword} />
    </ConfirmDialog>
  );
}
