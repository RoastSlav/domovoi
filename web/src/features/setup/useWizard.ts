import { useState } from "react";
import { useTestConnection, type Changes, type SecretChange } from "../../api/hooks/settings";
import { ApiFailure, type Editable } from "../../api/types";
import {
  buildChanges,
  changedPaths,
  getIn,
  isSensitive,
  setIn,
  sourceOf,
  toInput,
} from "../settings/form";
import { useApplySettings } from "../settings/useApply";
import { STEPS, browserTimeZone, type Step } from "./steps";

/** How one step ended, for the summary: how many settings it saved, or that it was skipped. */
export interface StepOutcome {
  changed: number;
  skipped: boolean;
  step: Step;
}

type Inputs = Record<string, string | boolean>;

/**
 * The state of the first run: what is typed, which steps are skipped, where the person is,
 * testing a step's connection, and the one save at the end. Nothing is sent until the last
 * step; a skipped step's typed values are never sent.
 */
export function useWizard(editable: Editable) {
  const values = editable.values as Record<string, unknown>;
  // What the hub has now; it does not change while the wizard runs.
  const [baseline] = useState<Inputs>(() =>
    Object.fromEntries(
      STEPS.flatMap((s) => s.fields)
        .filter((f) => f.kind !== "secret")
        .map((f) => [f.path, toInput(getIn(values, f.path), f.kind)]),
    ),
  );
  const [inputs, setInputs] = useState<Inputs>(() => ({
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
  const [saveError, setSaveError] = useState<unknown>();
  const [pending, setPending] = useState<Changes>({});
  const apply = useApplySettings();
  const tester = useTestConnection(STEPS[step]?.test ?? "heartbeat");

  /** The changes of every step not in `without`, or of step `only` alone. */
  const changes = (without: Set<number> = skipped, only?: number): Changes => {
    const kept = STEPS.filter((_, i) => (only === undefined ? !without.has(i) : i === only));
    const out = buildChanges(
      kept.flatMap((s) => s.fields),
      baseline,
      inputs,
      secrets,
    );
    const token = secrets["discord.bot_token"];
    const discordKept = only === undefined ? !without.has(1) : only === 1;
    if (discordKept && token && "set" in token) {
      setIn(out, "discord.enabled", true);
    }
    return out;
  };

  const testStep = () => {
    const current = changes(skipped, step);
    // A Discord test also needs the public URL, which the hub step holds.
    const isDiscord = STEPS[step]?.test === "discord";
    tester.mutate(isDiscord ? { ...changes(skipped, 0), ...current } : current);
  };

  const save = async (toSave: Changes, password?: string) => {
    setSaveError(undefined);
    setPasswordError(undefined);
    try {
      await apply.run({
        base_revision: editable.revision,
        changes: toSave,
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
      setSaveError(error);
    }
  };

  /** Moves on from the current step, marking it skipped or not; the last one saves. */
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
    const everything = changes(now);
    setPending(everything);
    if (isSensitive(everything)) {
      setAsking(true);
    } else {
      void save(everything);
    }
  };

  const back = () => {
    setStep(step - 1);
    tester.reset();
  };

  const outcomes = (): StepOutcome[] =>
    STEPS.map((s, i) => ({
      step: s,
      skipped: skipped.has(i),
      changed: skipped.has(i) ? 0 : changedPaths(changes(skipped, i)).length,
    }));

  return {
    apply,
    asking,
    back,
    cancelPassword: () => setAsking(false),
    changes,
    confirmPassword: (password: string) => void save(pending, password),
    inputs,
    next,
    outcomes,
    passwordError,
    saveError,
    secrets,
    setInput: (path: string, value: string | boolean) =>
      setInputs((now) => ({ ...now, [path]: value })),
    setSecret: (path: string, change: SecretChange) =>
      setSecrets((now) => ({ ...now, [path]: change })),
    skipped,
    step,
    testStep,
    tester,
  };
}

/** The wizard's state, as `useWizard` returns it. */
export type Wizard = ReturnType<typeof useWizard>;
