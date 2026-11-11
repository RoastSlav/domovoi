import { useId, useState, type FormEvent } from "react";
import { useCreateAckRule } from "../../api/hooks/rules";
import {
  Button,
  Dialog,
  ErrorDetail,
  Field,
  InlineMessage,
  Select,
  Stack,
  TextInput,
  useActionFeedback,
} from "../../components";
import { EMPTY, FOR, FROM, loginBody, type Fields, type LoginProblems } from "./sshRules";

/**
 * Marks a login as expected on `host`: a user (or anyone), where it comes from, how long
 * the expectation lasts and why. The rule is created when the form is sent; a mistake marks
 * its field and a failure stays in the dialog.
 */
export function AddLoginDialog({ host, onClose }: { host: string; onClose: () => void }) {
  const formId = useId();
  const create = useActionFeedback(useCreateAckRule(), {
    success: "Expected login added",
    error: "add the expected login",
    errorDisplay: "inline",
  });
  const [fields, setFields] = useState<Fields>(EMPTY);
  const [problems, setProblems] = useState<LoginProblems>({});
  const change = (patch: Partial<Fields>) => {
    setFields({ ...fields, ...patch });
    setProblems({});
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const built = loginBody(host, fields);
    if (!("body" in built)) {
      setProblems(built);
      return;
    }
    create.mutate(built.body, { onSuccess: onClose });
  };

  return (
    <Dialog
      title={`Add an expected login on ${host}`}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form={formId} loading={create.isPending}>
            Add expected login
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} noValidate>
        <Stack gap={4}>
          <Field label="User" hint="Leave it empty to expect any user.">
            <TextInput
              value={fields.user}
              placeholder="alice"
              onChange={(event) => change({ user: event.target.value })}
            />
          </Field>
          <Field label="From">
            <Select value={fields.from} onChange={(event) => change({ from: event.target.value })}>
              {FROM.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </Select>
          </Field>
          {fields.from === "range" && (
            <Field
              label="Address or range"
              hint="One address, or a range in CIDR form."
              error={problems.cidr}
            >
              <TextInput
                value={fields.cidr}
                placeholder="192.168.1.20 or 192.168.1.0/24"
                onChange={(event) => change({ cidr: event.target.value })}
              />
            </Field>
          )}
          <Field label="For" hint="After this the login alerts again.">
            <Select value={fields.life} onChange={(event) => change({ life: event.target.value })}>
              {FOR.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Note" hint="Why the login is expected." error={problems.note}>
            <TextInput
              value={fields.note}
              onChange={(event) => change({ note: event.target.value })}
            />
          </Field>
          {create.error && (
            <InlineMessage tone="danger" title={create.error.title}>
              <ErrorDetail error={create.error} />
            </InlineMessage>
          )}
        </Stack>
      </form>
    </Dialog>
  );
}
