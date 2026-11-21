import { useState } from "react";
import { useAssertRelation } from "../../api/hooks/knowledgeWrites";
import { Button, Dialog, Field, Select, Stack, useActionFeedback } from "../../components";
import { RELATION_KINDS } from "../../design/knowledgeText";
import { FailureMessage } from "./FailureMessage";
import { SubjectField } from "./SubjectField";

const ANY_SUBJECT = ["host", "container", "unit", "entity"] as const;

/** The add-a-relation form: from, kind and to. The hub's 400 reads inside the dialog. */
export function RelationForm({ onClose }: { onClose: () => void }) {
  const [from, setFrom] = useState("");
  const [kind, setKind] = useState("runs_on");
  const [to, setTo] = useState("");
  const assert = useAssertRelation();
  const feedback = useActionFeedback(assert, {
    success: "Relation saved.",
    error: "save the relation",
    errorDisplay: "inline",
  });
  const missing = from === "" || to === "" ? "Choose both ends of the relation." : undefined;
  return (
    <Dialog
      title="Add a relation"
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={feedback.isPending}
            disabledReason={missing}
            onClick={() => feedback.mutate({ from, kind, to }, { onSuccess: onClose })}
          >
            Save relation
          </Button>
        </>
      }
    >
      <Stack gap={4}>
        <SubjectField label="From" kinds={ANY_SUBJECT} value={from} onChange={setFrom} />
        <Field label="Relation">
          <Select value={kind} onChange={(event) => setKind(event.target.value)}>
            {Object.entries(RELATION_KINDS).map(([key, phrase]) => (
              <option key={key} value={key}>
                {phrase}
              </option>
            ))}
          </Select>
        </Field>
        <SubjectField label="To" kinds={ANY_SUBJECT} value={to} onChange={setTo} />
        <FailureMessage error={feedback.error} />
      </Stack>
    </Dialog>
  );
}
