import { useState } from "react";
import { useCreateConversation } from "../../api/hooks/conversations";
import { useHosts } from "../../api/hooks/hosts";
import type { ScopeChoice } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  Field,
  InlineMessage,
  SegmentedControl,
  Select,
  Tag,
  useActionFeedback,
} from "../../components";
import styles from "./operator.module.css";

type Kind = "" | "host" | "global";

/**
 * Starts a conversation. The scope is chosen here and never follows the top-bar selector
 * afterwards: with a host selected "One host" starts chosen, otherwise nothing is, so "all
 * hosts" is never an accident.
 */
export function NewConversation({
  operation,
  initialHost,
  onCreated,
}: {
  initialHost?: string | undefined;
  onCreated: (id: string) => void;
  operation?: string | undefined;
}) {
  const [selected] = useSelectedHost();
  const start = initialHost ?? selected;
  const hosts = useHosts();
  const [kind, setKind] = useState<Kind>(start ? "host" : "");
  const [host, setHost] = useState<string | undefined>(start);
  const create = useActionFeedback(useCreateConversation(), {
    success: () => null,
    error: "start the conversation",
    errorDisplay: "inline",
  });
  const names = (hosts.data ?? []).map((h) => h.name);
  const chosen = host ?? names[0];

  const reason =
    kind === ""
      ? "Choose what the conversation is about first."
      : kind === "host" && chosen === undefined
        ? "There is no host to talk about yet."
        : undefined;

  const submit = () => {
    const scope: ScopeChoice =
      kind === "host" ? { kind: "host", host: chosen ?? null } : { kind: "global" };
    create.mutate(
      { scope, operation_id: operation ?? null },
      { onSuccess: (detail) => onCreated(detail.id) },
    );
  };

  return (
    <div className={styles.form}>
      <Field
        label="This conversation is about"
        hint="It stays about this, whatever the host selector says."
      >
        <SegmentedControl<Kind>
          label="This conversation is about"
          value={kind}
          onChange={setKind}
          options={[
            { value: "host", label: "One host" },
            { value: "global", label: "All hosts" },
          ]}
        />
      </Field>
      {kind === "host" && (
        <Field label="Host">
          <Select value={chosen ?? ""} onChange={(event) => setHost(event.target.value)}>
            {names.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </Select>
        </Field>
      )}
      {operation && <Tag>Operation attached, read-only</Tag>}
      {create.error && (
        <InlineMessage tone="danger" title={create.error.title}>
          {create.error.detail}
        </InlineMessage>
      )}
      <div>
        <Button loading={create.isPending} disabledReason={reason} onClick={submit}>
          Start conversation
        </Button>
      </div>
    </div>
  );
}
