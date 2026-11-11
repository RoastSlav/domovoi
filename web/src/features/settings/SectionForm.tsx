import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState, type FormEvent, type ReactNode } from "react";
import {
  useTestConnection,
  type ProviderSlot,
  type SecretChange,
  type TestTarget,
} from "../../api/hooks/settings";
import { keys } from "../../api/queryClient";
import { ApiFailure, type Editable, type Issue } from "../../api/types";
import { Button, InlineMessage, PasswordPrompt } from "../../components";
import { describeError } from "../../design/messages";
import type { Tone } from "../../design/tone";
import { Field, ModelField, SecretField } from "./fields";
import {
  buildChanges,
  describe,
  getIn,
  inputProblem,
  isSecretSet,
  isSensitive,
  issuesByPath,
  lockedBy,
  sourceOf,
  toInput,
  type FieldSpec,
} from "./form";
import styles from "./settings.module.css";
import { TestIssues } from "./TestIssues";
import { phaseText, useApplySettings } from "./useApply";

/** What a section edits and offers. */
export interface SectionProps {
  title: string;
  fields: FieldSpec[];
  editable: Editable;
  schema?: Record<string, unknown> | undefined;
  /** A connection test offered in this section. */
  test?: TestTarget;
  /** Which provider the test above targets; the main one by default. */
  testSlot?: ProviderSlot;
  /** Shown above the fields. */
  intro?: ReactNode;
}

const TEST_LABEL: Record<TestTarget, string> = {
  discord: "Send a test message",
  llm: "Test the model",
  heartbeat: "Ping the heartbeat URL",
};

interface SaveFailure {
  detail?: string | undefined;
  issues: Issue[];
  stale: boolean;
  title: string;
}

function failureOf(error: unknown): SaveFailure {
  const described = describeError(error, "save the settings");
  const stale =
    error instanceof ApiFailure &&
    error.status === 409 &&
    error.message.includes("changed elsewhere");
  return stale
    ? {
        title: "Settings changed elsewhere",
        detail: "Reload to see the current values, then make your change again.",
        issues: [],
        stale,
      }
    : { title: described.title, detail: described.detail, issues: described.issues ?? [], stale };
}

const PHASE_TONE: Record<string, Tone> = {
  applying: "progress",
  applied: "success",
  saved: "info",
  slow: "warning",
};

/**
 * One settings section: its fields, a Save and apply button, and a connection test when
 * it has one. Fields set in hub.toml or the environment are read-only with their source;
 * secrets are write-only; the hub's validation errors show next to their fields.
 */
export function SectionForm({
  title,
  fields,
  editable,
  schema,
  test,
  testSlot,
  intro,
}: SectionProps) {
  const values = editable.values as Record<string, unknown>;
  const initial = useMemo(
    () =>
      Object.fromEntries(
        fields
          .filter((f) => f.kind !== "secret")
          .map((f) => [f.path, toInput(getIn(values, f.path), f.kind)]),
      ),
    [fields, values],
  );
  const [inputs, setInputs] = useState<Record<string, string | boolean>>(initial);
  const [secrets, setSecrets] = useState<Record<string, SecretChange>>({});
  const [problem, setProblem] = useState<ReturnType<typeof failureOf>>();
  const [asking, setAsking] = useState(false);
  const [passwordError, setPasswordError] = useState<string>();
  const apply = useApplySettings();
  const tester = useTestConnection(test ?? "heartbeat", testSlot ?? "primary");
  const client = useQueryClient();
  const phase = phaseText(apply.phase);
  const id = `section-${title.toLowerCase().replace(/\W+/g, "-")}`;

  const visibleFields = fields.filter(
    (field) =>
      !field.visibleWhen ||
      field.visibleWhen.values.includes(String(inputs[field.visibleWhen.path] ?? "")),
  );
  const local = new Map<string, string[]>();
  for (const field of visibleFields) {
    const input = inputs[field.path];
    const bad = input === undefined ? undefined : inputProblem(input, field.kind);
    if (bad) {
      local.set(field.path, [bad]);
    }
  }
  const fromHub = issuesByPath(problem?.issues ?? []);
  const shown = new Set(visibleFields.map((f) => f.path));
  const unplaced = (problem?.issues ?? []).filter((i) => !shown.has(i.path));
  const changes = () => buildChanges(fields, initial, inputs, secrets);

  const submit = async (password?: string) => {
    setProblem(undefined);
    setPasswordError(undefined);
    try {
      await apply.run({
        base_revision: editable.revision,
        changes: changes(),
        ...(password ? { password } : {}),
      });
      setAsking(false);
      setSecrets({});
    } catch (error) {
      if (asking && error instanceof ApiFailure && error.status === 403) {
        setPasswordError("That password is not right.");
        return;
      }
      setAsking(false);
      setProblem(failureOf(error));
    }
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (local.size > 0) {
      return;
    }
    if (isSensitive(changes())) {
      setAsking(true);
    } else {
      void submit();
    }
  };

  return (
    <section className={styles.section} aria-labelledby={id}>
      <h2 id={id}>{title}</h2>
      {intro}
      <form className={styles.form} onSubmit={onSubmit} noValidate>
        {visibleFields.map((spec) => {
          const locked = lockedBy(sourceOf(editable.fields, spec.path));
          const hint = spec.hint ?? describe(schema, spec.path);
          const errors = [...(local.get(spec.path) ?? []), ...(fromHub.get(spec.path) ?? [])];
          return spec.kind === "secret" ? (
            <SecretField
              key={spec.path}
              spec={spec}
              hint={hint}
              locked={locked}
              errors={errors}
              isSet={isSecretSet(values, spec.path)}
              change={secrets[spec.path] ?? { keep: true }}
              onChange={(change) => setSecrets((s) => ({ ...s, [spec.path]: change }))}
            />
          ) : spec.kind === "model" ? (
            <ModelField
              key={spec.path}
              spec={spec}
              hint={hint}
              locked={locked}
              errors={errors}
              value={String(inputs[spec.path] ?? "")}
              candidate={changes}
              slot={spec.slot}
              onChange={(value) => setInputs((s) => ({ ...s, [spec.path]: value }))}
            />
          ) : (
            <Field
              key={spec.path}
              spec={spec}
              hint={hint}
              locked={locked}
              errors={errors}
              value={inputs[spec.path] ?? ""}
              onChange={(value) => setInputs((s) => ({ ...s, [spec.path]: value }))}
            />
          );
        })}
        {problem && (
          <InlineMessage
            tone="danger"
            title={problem.title}
            action={
              problem.stale && (
                <Button
                  size="sm"
                  icon="refresh"
                  onClick={() => void client.invalidateQueries({ queryKey: keys.settings })}
                >
                  Reload
                </Button>
              )
            }
          >
            {problem.detail}
            {unplaced.map((issue) => (
              <p key={`${issue.path}:${issue.message}`}>
                {issue.path ? `${issue.path}: ` : ""}
                {issue.message}
              </p>
            ))}
          </InlineMessage>
        )}
        <div className={styles.actions}>
          <Button type="submit" variant="primary" loading={apply.pending}>
            Save and apply
          </Button>
          {test && (
            <Button loading={tester.isPending} onClick={() => tester.mutate(changes())}>
              {testSlot === "secondary" ? "Test the fallback model" : TEST_LABEL[test]}
            </Button>
          )}
        </div>
        {phase && <InlineMessage tone={PHASE_TONE[apply.phase] ?? "info"} title={phase} />}
        {tester.data && (
          <InlineMessage
            tone={tester.data.ok ? "success" : "danger"}
            title={tester.data.ok ? "The test worked" : "The test failed"}
          >
            {tester.data.message} ({tester.data.latency_ms} ms)
          </InlineMessage>
        )}
        <TestIssues error={tester.error} />
      </form>
      {asking && (
        <PasswordPrompt
          reason="This change affects who can act or what is trusted. Enter your password to save it."
          error={passwordError}
          busy={apply.pending}
          onConfirm={(password) => void submit(password)}
          onCancel={() => setAsking(false)}
        />
      )}
    </section>
  );
}
