import { useState } from "react";
import { useNavigate } from "react-router";
import { useSettings } from "../../api/hooks/system";
import { useTestConnection, type SecretChange, type TestTarget } from "../../api/hooks/settings";
import { ApiFailure, type Editable, type Issue } from "../../api/types";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { PasswordPrompt } from "../../components/PasswordPrompt";
import { EnrollDialog } from "../hosts/EnrollDialog";
import { Field, SecretField } from "../settings/fields";
import {
  buildChanges,
  getIn,
  isSecretSet,
  isSensitive,
  lockedBy,
  setIn,
  sourceOf,
  toInput,
  type FieldSpec,
} from "../settings/form";
import styles from "../settings/settings.module.css";
import { phaseText, useApplySettings } from "../settings/useApply";

interface Step {
  title: string;
  intro: string;
  fields: FieldSpec[];
  test?: TestTarget;
  optional: boolean;
}

/** The steps before the first host; their settings are applied together. */
export const STEPS: Step[] = [
  {
    title: "Hub",
    intro: "What this hub is called and where people reach it.",
    optional: false,
    fields: [
      { path: "hub.name", label: "Name", kind: "text" },
      { path: "hub.timezone", label: "Time zone", kind: "text" },
      { path: "web.public_url", label: "Public URL", kind: "text" },
    ],
  },
  {
    title: "Discord",
    intro: "Incidents arrive as cards in a channel, with buttons to acknowledge and approve.",
    optional: true,
    test: "discord",
    fields: [
      { path: "discord.bot_token", label: "Bot token", kind: "secret" },
      { path: "discord.channel_id", label: "Channel id", kind: "ids" },
      { path: "discord.allowed_user_ids", label: "Users who may approve", kind: "ids" },
    ],
  },
  {
    title: "Model",
    intro: "A model triages each incident: what happened, why, and what to do.",
    optional: true,
    test: "llm",
    fields: [
      {
        path: "llm.primary.kind",
        label: "Provider",
        kind: "select",
        options: ["", "anthropic", "openai_compatible", "cli"],
      },
      { path: "llm.primary.model", label: "Model", kind: "text" },
      { path: "llm.primary.api_key", label: "API key", kind: "secret" },
      {
        path: "llm.primary.cli.preset",
        label: "CLI preset",
        kind: "select",
        options: ["", "claude", "codex"],
      },
      { path: "triage.monthly_budget_usd", label: "Monthly budget (USD)", kind: "number" },
      { path: "triage.enabled", label: "Triage incidents with this model", kind: "bool" },
    ],
  },
  {
    title: "Digest and quiet hours",
    intro: "A daily summary, and hours when only critical alerts get through.",
    optional: true,
    fields: [
      { path: "notify.digest.time", label: "Digest time", kind: "text", placeholder: "09:00" },
      {
        path: "notify.quiet_hours.start",
        label: "Quiet hours start",
        kind: "text",
        placeholder: "00:00",
      },
      {
        path: "notify.quiet_hours.end",
        label: "Quiet hours end",
        kind: "text",
        placeholder: "09:00",
      },
    ],
  },
];

/** The step titles shown in the progress line, the first host included. */
const TITLES = [...STEPS.map((s) => s.title), "First host"];

function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return "UTC";
  }
}

function Wizard({ editable }: { editable: Editable }) {
  const values = editable.values as Record<string, unknown>;
  const all = STEPS.flatMap((s) => s.fields);
  // What the hub has now; it does not change while the wizard runs.
  const [baseline] = useState<Record<string, string | boolean>>(() =>
    Object.fromEntries(
      all
        .filter((f) => f.kind !== "secret")
        .map((f) => [f.path, toInput(getIn(values, f.path), f.kind)]),
    ),
  );
  const [inputs, setInputs] = useState<Record<string, string | boolean>>(() => ({
    ...baseline,
    "hub.timezone":
      getIn(values, "hub.timezone") && sourceOf(editable.fields, "hub.timezone") !== "default"
        ? (baseline["hub.timezone"] ?? "")
        : browserTimeZone(),
    "web.public_url": baseline["web.public_url"] || window.location.origin,
    "triage.enabled": true,
  }));
  const [secrets, setSecrets] = useState<Record<string, SecretChange>>({});
  const [step, setStep] = useState(0);
  const [skipped, setSkipped] = useState<Set<number>>(new Set());
  const [asking, setAsking] = useState(false);
  const [passwordError, setPasswordError] = useState<string>();
  const [issues, setIssues] = useState<Issue[]>([]);
  const [failure, setFailure] = useState<string>();
  const [adding, setAdding] = useState(false);
  const apply = useApplySettings();
  const tester = useTestConnection(STEPS[step]?.test ?? "heartbeat");
  const navigate = useNavigate();

  const changes = (without: Set<number> = skipped) => {
    const kept = STEPS.filter((_, i) => !without.has(i)).flatMap((s) => s.fields);
    const out = buildChanges(kept, baseline, inputs, secrets);
    const token = secrets["discord.bot_token"];
    if (!without.has(1) && token && "set" in token) {
      setIn(out, "discord.enabled", true);
    }
    return out;
  };

  const save = async (pending: ReturnType<typeof changes>, password?: string) => {
    setFailure(undefined);
    setIssues([]);
    setPasswordError(undefined);
    try {
      await apply.run({
        base_revision: editable.revision,
        changes: pending,
        ...(password ? { password } : {}),
      });
      setAsking(false);
      setStep(STEPS.length);
    } catch (error) {
      if (asking && error instanceof ApiFailure && error.status === 403) {
        setPasswordError("That password is not right.");
        return;
      }
      setAsking(false);
      setFailure(
        error instanceof ApiFailure ? error.message : "Could not save. Is the hub running?",
      );
      setIssues(error instanceof ApiFailure ? error.issues : []);
    }
  };

  const [pending, setPending] = useState<ReturnType<typeof changes>>({});

  const next = (skip: boolean) => {
    const now = new Set(skipped);
    if (skip) {
      now.add(step);
    } else {
      now.delete(step);
    }
    setSkipped(now);
    if (step < STEPS.length - 1) {
      setStep(step + 1);
      tester.reset();
      return;
    }
    const all = changes(now);
    setPending(all);
    if (isSensitive(all)) {
      setAsking(true);
    } else {
      void save(all);
    }
  };

  const current = STEPS[step];
  return (
    <>
      <h1>Set up Domovoi</h1>
      <ol className={styles.progress} aria-label="Setup steps">
        {TITLES.map((title, i) => (
          <li key={title} aria-current={i === step ? "step" : undefined}>
            {title}
            {skipped.has(i) ? " (skipped)" : ""}
          </li>
        ))}
      </ol>
      {current ? (
        <section aria-labelledby="step-heading" className={styles.form}>
          <h2 id="step-heading">{current.title}</h2>
          <p>{current.intro}</p>
          {current.fields.map((spec) => {
            const locked = lockedBy(sourceOf(editable.fields, spec.path));
            const errors = issues.filter((i) => i.path === spec.path).map((i) => i.message);
            return spec.kind === "secret" ? (
              <SecretField
                key={spec.path}
                spec={spec}
                locked={locked}
                errors={errors}
                isSet={isSecretSet(values, spec.path)}
                change={secrets[spec.path] ?? { keep: true }}
                onChange={(change) => setSecrets((s) => ({ ...s, [spec.path]: change }))}
              />
            ) : (
              <Field
                key={spec.path}
                spec={spec}
                locked={locked}
                errors={errors}
                value={inputs[spec.path] ?? ""}
                onChange={(value) => setInputs((s) => ({ ...s, [spec.path]: value }))}
              />
            );
          })}
          {failure && (
            <div role="alert" className={styles.alert}>
              <p className={styles.status}>{failure}</p>
              {issues.map((i) => (
                <p key={`${i.path}:${i.message}`} className={styles.status}>
                  {i.path ? `${i.path}: ` : ""}
                  {i.message}
                </p>
              ))}
            </div>
          )}
          <div className={styles.actions}>
            {step > 0 && <Button onClick={() => setStep(step - 1)}>Back</Button>}
            {current.test && (
              <Button disabled={tester.isPending} onClick={() => tester.mutate(changes())}>
                Test
              </Button>
            )}
            {current.optional && (
              <Button disabled={apply.pending} onClick={() => next(true)}>
                Skip
              </Button>
            )}
            <Button variant="primary" disabled={apply.pending} onClick={() => next(false)}>
              {step === STEPS.length - 1 ? "Save and apply" : "Next"}
            </Button>
          </div>
          {tester.data && (
            <p role="status" className={styles.status}>
              {tester.data.ok ? "Worked: " : "Failed: "}
              {tester.data.message}
            </p>
          )}
          {phaseText(apply.phase) && (
            <p role="status" className={styles.status}>
              {phaseText(apply.phase)}
            </p>
          )}
        </section>
      ) : (
        <section aria-labelledby="host-heading" className={styles.form}>
          <h2 id="host-heading">Add your first host</h2>
          {phaseText(apply.phase) && (
            <p role="status" className={styles.status}>
              {phaseText(apply.phase)}
            </p>
          )}
          <p>
            Install the agent on a machine you want watched, then enroll it with a one-time command.
          </p>
          <div className={styles.actions}>
            <Button onClick={() => setAdding(true)}>Add a host</Button>
            <Button
              variant="primary"
              onClick={() => void navigate("/incidents", { replace: true })}
            >
              Finish
            </Button>
          </div>
          {adding && <EnrollDialog onClose={() => setAdding(false)} />}
        </section>
      )}
      {asking && (
        <PasswordPrompt
          reason="Saving a token or key needs your password again."
          error={passwordError}
          busy={apply.pending}
          onConfirm={(password) => void save(pending, password)}
          onCancel={() => setAsking(false)}
        />
      )}
    </>
  );
}

/**
 * The guided first run after the admin account exists: hub basics, Discord, a model, the
 * digest and quiet hours, applied together, then the first host. Every step but the first
 * can be skipped; the Settings page has all of it later.
 */
export function FirstRun() {
  const settings = useSettings();
  if (settings.isPending) {
    return <p>Loading…</p>;
  }
  if (settings.error || !settings.data.editable) {
    return <EmptyState title="Settings are managed in hub.toml on this hub" />;
  }
  return <Wizard editable={settings.data.editable} />;
}
