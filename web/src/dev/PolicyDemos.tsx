import { useState } from "react";
import type { PolicyRule } from "../api/types";
import { Button, Cluster, PasswordPrompt, Stack, StatusPill, Text } from "../components";
import { POLICY_VOCABULARIES } from "../design/policyMeta";
import { DecisionExplanation } from "../features/policy/decision/DecisionExplanation";
import { decisionOf } from "../features/policy/decisionModel";
import { OutcomePill, RuleMarks } from "../features/policy/RuleMarks";
import {
  DECISION_ERROR,
  DECISION_EXCEPTION,
  DECISION_LIMITS,
  DECISION_LOCKED,
  DECISION_UNAVAILABLE,
  VERSION_3,
} from "../test/policyFixtures";
import { Demo, Group } from "./Demo";
import styles from "./design.module.css";

const BASE_RULE = VERSION_3.rules[0] as PolicyRule;

function rule(overrides: Partial<PolicyRule>): PolicyRule {
  return { ...BASE_RULE, ...overrides };
}

const MARKED: { name: string; rule: PolicyRule }[] = [
  { name: "plain rule (no marks)", rule: rule({ locked: false, shadow: false, enabled: true }) },
  { name: "locked Deny", rule: rule({ locked: true, outcome: "deny", shadow: false }) },
  { name: "shadow", rule: rule({ locked: false, shadow: true, enabled: true }) },
  { name: "disabled", rule: rule({ locked: false, shadow: false, enabled: false }) },
  {
    name: "all marks",
    rule: rule({ locked: true, outcome: "deny", shadow: true, enabled: false }),
  },
];

const OUTCOMES = ["allow", "require_approval", "deny", "unavailable", "error", "other"];

const EXPLANATIONS = [
  { title: "Allow with constraints", detail: DECISION_EXCEPTION },
  { title: "Locked Deny", detail: DECISION_LOCKED },
  { title: "Hard limits and gaps", detail: DECISION_LIMITS },
  { title: "Unavailable", detail: DECISION_UNAVAILABLE },
  { title: "Error", detail: DECISION_ERROR },
];

/** The policy pages' vocabularies, rule marks, decision explanations and password prompt. */
export function PolicyDemos() {
  const [open, setOpen] = useState(false);
  return (
    <Group title="Policy">
      <Demo name="StatusPill (every policy vocabulary; the last of each is an unknown value)">
        <Stack gap={2}>
          {POLICY_VOCABULARIES.map((vocabulary) => (
            <div key={vocabulary.name} className={styles.row}>
              <h3>{vocabulary.name}</h3>
              <Cluster>
                {[...vocabulary.values, "unknown_value"].map((value) => (
                  <StatusPill key={value} meta={vocabulary.meta(value)} />
                ))}
              </Cluster>
            </div>
          ))}
        </Stack>
      </Demo>
      <Demo name="OutcomePill and RuleMarks">
        <Stack gap={2}>
          <Cluster>
            {OUTCOMES.map((outcome) => (
              <OutcomePill key={outcome} outcome={outcome} />
            ))}
          </Cluster>
          {MARKED.map((sample) => (
            <Cluster key={sample.name}>
              <Text>{sample.name}</Text>
              <OutcomePill outcome={sample.rule.outcome} />
              <RuleMarks rule={sample.rule} />
            </Cluster>
          ))}
        </Stack>
      </Demo>
      <Demo name="DecisionExplanation (enforced, then shadow, of each recorded decision)">
        <Stack gap={4}>
          {EXPLANATIONS.map(({ title, detail }) => (
            <Stack key={title} gap={2}>
              <DecisionExplanation
                title={`${title}, enforced`}
                decision={decisionOf(detail.enforced)}
                zone="Europe/Berlin"
              />
              <DecisionExplanation
                title={`${title}, shadow`}
                decision={decisionOf(detail.shadow)}
                zone="Europe/Berlin"
                emptyText="No shadow rule matched."
              />
            </Stack>
          ))}
        </Stack>
      </Demo>
      <Demo name="PasswordPrompt with content of its own">
        <Button onClick={() => setOpen(true)}>Open PasswordPrompt</Button>
        {open && (
          <PasswordPrompt
            reason="Publishing this version widens what the agent may do on its own."
            confirmLabel="Publish"
            onConfirm={() => setOpen(false)}
            onCancel={() => setOpen(false)}
          >
            <Text block>Two conflicts were acknowledged: restarts and prunes overlap.</Text>
          </PasswordPrompt>
        )}
      </Demo>
    </Group>
  );
}
