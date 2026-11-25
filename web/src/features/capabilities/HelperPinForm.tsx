import { useState } from "react";
import { usePinHelperKey } from "../../api/hooks/helper";
import {
  Button,
  CopyBlock,
  Field,
  InlineMessage,
  PasswordInput,
  Stack,
  Text,
  TextInput,
  useActionFeedback,
} from "../../components";
import { FINGERPRINT_COMMAND, normalizeFingerprint } from "./helperText";

/**
 * Pins the key the hub received. The owner types the fingerprint the host prints, so the
 * comparison is theirs; the hub checks the typed value against the whole key and asks for the
 * password again. A wrong password stays here, in words, and the session stays open.
 */
export function HelperPinForm({ host, replacing }: { host: string; replacing: boolean }) {
  const [typed, setTyped] = useState("");
  const [password, setPassword] = useState("");
  const pin = useActionFeedback(usePinHelperKey(host), {
    success: `Pinned the helper key on ${host}. The next report is checked against it.`,
    error: "pin the helper key",
    errorDisplay: "inline",
  });
  const complete = normalizeFingerprint(typed).length === 64;
  const reason = !complete
    ? "Type the whole fingerprint the host prints, 64 characters."
    : password === ""
      ? "Enter your password first."
      : undefined;
  return (
    <Stack gap={3}>
      <Text block>
        On the host, run this as root. Then <strong>type</strong> what it prints into the box below,
        so you compare it yourself rather than copying it from this page.
      </Text>
      <CopyBlock label="Command to run on the host as root" text={FINGERPRINT_COMMAND} />
      <Field label="Fingerprint printed on the host" hint="Spaces and capitals do not matter.">
        <TextInput
          value={typed}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setTyped(event.target.value)}
        />
      </Field>
      <Field label="Your password" hint="Pinning a key widens what the hub relies on.">
        <PasswordInput
          value={password}
          autoComplete="current-password"
          onChange={(event) => setPassword(event.target.value)}
        />
      </Field>
      {pin.error && (
        <InlineMessage tone="danger" title={pin.error.title}>
          {pin.error.detail}
        </InlineMessage>
      )}
      <div>
        <Button
          variant="primary"
          icon="lock"
          loading={pin.isPending}
          disabledReason={reason}
          onClick={() => pin.mutate({ fingerprint: typed, password })}
        >
          {replacing ? "Pin the new key" : "Pin this key"}
        </Button>
      </div>
    </Stack>
  );
}
