import { useState, type FormEvent } from "react";
import { useCreateInvite, useHosts } from "../../api/hooks/hosts";
import type { Invite } from "../../api/types";
import {
  Button,
  Dialog,
  ErrorDetail,
  Field,
  InlineMessage,
  Stack,
  Text,
  TextInput,
  useActionFeedback,
} from "../../components";
import { InviteSteps, Step } from "./EnrollInvite";
import styles from "./hostsList.module.css";

/**
 * Adding a host as three steps: create an invite for a name, copy the command it gives to
 * run on that machine, then wait while this page watches for the agent to connect.
 */
export function EnrollDialog({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [invite, setInvite] = useState<Invite>();
  // The hosts there were before the invite, so the one that connects can be told apart.
  const [known, setKnown] = useState<ReadonlySet<string>>();
  const hosts = useHosts(false);
  // The dialog itself shows the new invite, so success needs no toast.
  const create = useActionFeedback(useCreateInvite(), {
    success: () => null,
    error: "create the invite",
    errorDisplay: "inline",
  });

  const request = () => {
    const before = hosts.data && new Set(hosts.data.map((host) => host.id));
    create.mutate(name.trim(), {
      onSuccess: (created) => {
        setKnown(before);
        setInvite(created);
      },
    });
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (name.trim()) {
      request();
    }
  };

  return (
    <Dialog
      title={invite ? `Enroll ${name.trim()}` : "Add a host"}
      onClose={onClose}
      wide
      actions={
        <>
          {invite && (
            <Button onClick={request} loading={create.isPending}>
              New invite
            </Button>
          )}
          <Button variant={invite ? "primary" : "secondary"} onClick={onClose}>
            {invite ? "Done" : "Cancel"}
          </Button>
        </>
      }
    >
      <ol className={styles.steps} role="list">
        <Step title="Create an invite" state={invite ? "done" : "active"}>
          {invite ? (
            <Text muted>Invite created for {name.trim()}.</Text>
          ) : (
            <form onSubmit={onSubmit}>
              <Stack gap={2}>
                <Field
                  label="Host name"
                  hint={
                    <>
                      Lower-case letters, digits and dashes, for example <code>nas</code> or{" "}
                      <code>pi-garage</code>.
                    </>
                  }
                >
                  <TextInput
                    mono
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </Field>
                <Button
                  type="submit"
                  variant="primary"
                  loading={create.isPending}
                  disabledReason={name.trim() ? undefined : "Enter a host name first."}
                >
                  Create invite
                </Button>
              </Stack>
            </form>
          )}
          {create.error && (
            <InlineMessage tone="danger" title={create.error.title}>
              <ErrorDetail error={create.error} />
            </InlineMessage>
          )}
        </Step>
        <InviteSteps invite={invite} known={known} />
      </ol>
    </Dialog>
  );
}
