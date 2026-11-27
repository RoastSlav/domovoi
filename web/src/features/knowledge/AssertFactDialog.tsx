import { useState } from "react";
import { useAssertFact } from "../../api/hooks/knowledgeWrites";
import {
  Button,
  Dialog,
  Field,
  InlineMessage,
  Select,
  Stack,
  useActionFeedback,
} from "../../components";
import { PREDICATES, needsPassword, predicateInfo } from "../../design/knowledgeText";
import { FailureMessage } from "./FailureMessage";
import { SubjectField } from "./SubjectField";
import { parseSubject } from "./subjects";
import { useStepUp } from "./useStepUp";
import { buildValue } from "./valueForm";
import { ValueFields, emptyDraft } from "./ValueFields";

const ASSERTABLE = Object.entries(PREDICATES).filter(([, info]) => info.assertable);

/** A fact to save: what it is about, its kind and its value. */
interface Assertion {
  predicate: string;
  subject: string;
  value: unknown;
}

/**
 * The add-a-fact form. A `preset` fixes the kind or the subject and hides that control. A kind
 * that affects protective actions swaps the form for the password prompt and back on Cancel;
 * the draft is kept.
 */
export function AssertFactDialog({
  onClose,
  preset,
}: {
  onClose: () => void;
  preset?: { predicate?: string; subject?: string };
}) {
  const [predicate, setPredicate] = useState(preset?.predicate ?? "role");
  const [subject, setSubject] = useState(preset?.subject ?? "");
  const [draft, setDraft] = useState(() => emptyDraft(preset?.predicate ?? "role"));
  const [problems, setProblems] = useState<{ subject?: string; value?: string }>({});
  const assert = useAssertFact();
  const feedback = useActionFeedback(assert, {
    success: "Fact saved.",
    error: "save the fact",
    errorDisplay: "inline",
  });
  const stepUp = useStepUp<Assertion>({
    confirmLabel: "Save",
    reason: () =>
      "This kind of fact can affect protective actions. Enter your password to save it.",
    run: (variables, password, callbacks) =>
      feedback.mutate(password === undefined ? variables : { ...variables, password }, {
        ...callbacks,
        onSuccess: (data) => {
          callbacks.onSuccess?.(data);
          onClose();
        },
      }),
    busy: feedback.isPending,
    error: feedback.error,
    reset: feedback.reset,
  });

  const info = predicateInfo(predicate);
  const pick = (next: string) => {
    setPredicate(next);
    setSubject(preset?.subject ?? "");
    setDraft(emptyDraft(next));
    setProblems({});
    feedback.reset();
  };

  const save = () => {
    const kind = parseSubject(subject)?.kind;
    const subjectProblem =
      kind && info.subjects.includes(kind) ? undefined : "Choose what it is about.";
    const built = buildValue({ ...draft, predicate });
    const valueProblem = built.ok ? undefined : built.problem;
    setProblems({
      ...(subjectProblem ? { subject: subjectProblem } : {}),
      ...(valueProblem ? { value: valueProblem } : {}),
    });
    if (subjectProblem || !built.ok) {
      return;
    }
    stepUp.start({ predicate, subject, value: built.value }, needsPassword("assert", predicate));
  };

  if (stepUp.dialog) {
    return stepUp.dialog;
  }
  // A management path is what the page calls it; any other kind is a fact.
  const path = preset?.predicate === "management_path";
  return (
    <Dialog
      title={path ? "Add a management path" : "Add a fact"}
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" loading={feedback.isPending} onClick={save}>
            {path ? "Save path" : "Save fact"}
          </Button>
        </>
      }
    >
      <Stack gap={4}>
        {!preset?.predicate && (
          <Field label="Kind" hint={info.hint}>
            <Select value={predicate} onChange={(event) => pick(event.target.value)}>
              {ASSERTABLE.map(([key, option]) => (
                <option key={key} value={key}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {!preset?.subject && (
          <>
            <SubjectField
              key={predicate}
              label="About"
              kinds={info.subjects}
              value={subject}
              onChange={setSubject}
            />
            {problems.subject && <InlineMessage tone="danger" title={problems.subject} />}
          </>
        )}
        <ValueFields
          draft={draft}
          problem={problems.value}
          onChange={(change) => setDraft((previous) => ({ ...previous, ...change }))}
        />
        <FailureMessage error={stepUp.inline} />
      </Stack>
    </Dialog>
  );
}
