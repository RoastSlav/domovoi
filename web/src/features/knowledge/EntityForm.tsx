import { useState } from "react";
import { useCreateEntity, useRenameEntity } from "../../api/hooks/knowledgeWrites";
import type { KnowledgeEntity } from "../../api/types";
import { Button, Dialog, Field, Stack, TextInput, useActionFeedback } from "../../components";
import { FailureMessage } from "./FailureMessage";

/** Creates an entity, or renames `entity` when one is given. The hub's 400 and 409 read inside. */
export function EntityForm({ entity, onClose }: { entity?: KnowledgeEntity; onClose: () => void }) {
  const [kind, setKind] = useState("");
  const [name, setName] = useState(entity?.name ?? "");
  const create = useActionFeedback(useCreateEntity(), {
    success: "Entity added.",
    error: "add the entity",
    errorDisplay: "inline",
  });
  const rename = useActionFeedback(useRenameEntity(), {
    success: "Entity renamed.",
    error: "rename the entity",
    errorDisplay: "inline",
  });
  const feedback = entity ? rename : create;
  const missing =
    name.trim() === ""
      ? "Enter a name first."
      : !entity && kind.trim() === ""
        ? "Enter a kind first."
        : undefined;
  const save = () => {
    if (entity) {
      rename.mutate(
        { id: entity.id, name: name.trim(), expectedVersion: entity.version },
        { onSuccess: onClose },
      );
    } else {
      create.mutate({ kind: kind.trim(), name: name.trim() }, { onSuccess: onClose });
    }
  };
  return (
    <Dialog
      title={entity ? `Rename ${entity.name}` : "Add an entity"}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={feedback.isPending}
            disabledReason={missing}
            onClick={save}
          >
            {entity ? "Rename" : "Add entity"}
          </Button>
        </>
      }
    >
      <Stack gap={4}>
        {!entity && (
          <Field label="Kind" hint="Lowercase letters, digits, - and _, up to 32.">
            <TextInput
              value={kind}
              autoComplete="off"
              onChange={(event) => setKind(event.target.value)}
            />
          </Field>
        )}
        <Field label="Name" hint="Up to 80 characters. Names are unique, ignoring case.">
          <TextInput
            value={name}
            autoComplete="off"
            onChange={(event) => setName(event.target.value)}
          />
        </Field>
        <FailureMessage error={feedback.error} />
      </Stack>
    </Dialog>
  );
}
