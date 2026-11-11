import { useState } from "react";
import { useCreateAckRule } from "../../api/hooks/rules";
import type { AckRule } from "../../api/types";
import {
  Button,
  Checkbox,
  ConfirmDialog,
  InlineMessage,
  RelativeTime,
  useActionFeedback,
} from "../../components";
import { loginWindow } from "./sshRules";

/**
 * "I’m logging in now": asks, then opens a one-hour window in which logins on `host` are
 * recorded and never alert. It covers the local network unless the box for the internet is
 * ticked.
 */
export function LoginWindow({ host }: { host: string }) {
  const [asking, setAsking] = useState(false);
  const [internet, setInternet] = useState(false);
  const create = useActionFeedback(useCreateAckRule(), {
    success: `Login window opened on ${host} for 1 hour`,
    error: "open the login window",
    errorDisplay: "inline",
  });
  const close = () => {
    setAsking(false);
    setInternet(false);
    create.reset();
  };
  return (
    <>
      <Button
        variant="primary"
        icon="clock"
        hint="Opens a one-hour window in which logins are recorded and never alert."
        onClick={() => setAsking(true)}
      >
        I&rsquo;m logging in now
      </Button>
      {asking && (
        <ConfirmDialog
          title={`Open a login window on ${host}?`}
          consequence={`For one hour, logins on ${host} by anyone are recorded and never alert you. Use it for the login you are about to make.`}
          confirmLabel="Open the window"
          busy={create.isPending}
          error={create.error}
          onConfirm={() => create.mutate(loginWindow(host, internet), { onSuccess: close })}
          onCancel={close}
        >
          <Checkbox
            label="Include logins from the internet"
            checked={internet}
            onChange={setInternet}
          />
        </ConfirmDialog>
      )}
    </>
  );
}

/** Says that a login window is open on `host`, what it covers and when it ends. */
export function LoginWindowNote({
  host,
  open,
}: {
  host: string;
  open: AckRule & { expires_at: string };
}) {
  return (
    <InlineMessage
      tone="info"
      title={`A login window is open on ${host}, so ${
        open.predicates.length === 0
          ? "any login, including from the internet, is expected"
          : "logins from the local network are expected"
      }.`}
    >
      It expires <RelativeTime at={open.expires_at} />.
    </InlineMessage>
  );
}
