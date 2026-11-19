import type { Table } from "./status";

/** The words for step kinds and edge kinds. */

/** What a step is for. */
export type StepKind =
  | "reasoning_turn"
  | "evidence_request"
  | "assessment"
  | "plan"
  | "policy_decision"
  | "approval"
  | "execution"
  | "verification"
  | "notification"
  | "question"
  | "note";

/** How two steps are linked. */
export type EdgeKind = "depends_on" | "caused_by" | "verifies" | "retry_of" | "answers";

export const STEP_KINDS: Table<StepKind> = {
  reasoning_turn: {
    label: "Model turn",
    tone: "neutral",
    icon: "chat",
    hint: "Domovoi asked the model to think about the evidence.",
  },
  evidence_request: {
    label: "Evidence",
    tone: "neutral",
    icon: "search",
    hint: "A read-only look at a host, such as logs or container state.",
  },
  assessment: {
    label: "Assessment",
    tone: "neutral",
    icon: "info",
    hint: "What Domovoi concluded from the evidence.",
  },
  plan: {
    label: "Plan",
    tone: "neutral",
    icon: "note",
    hint: "What Domovoi proposes to do.",
  },
  policy_decision: {
    label: "Policy decision",
    tone: "neutral",
    icon: "rules",
    hint: "How a policy rule judged an action.",
  },
  approval: {
    label: "Approval",
    tone: "neutral",
    icon: "user",
    hint: "A person decides whether an action may run.",
  },
  execution: {
    label: "Action",
    tone: "neutral",
    icon: "tool",
    hint: "An action that changes something on a host, once approved.",
  },
  verification: {
    label: "Check",
    tone: "neutral",
    icon: "check-circle",
    hint: "A follow-up check that the action fixed the problem.",
  },
  notification: {
    label: "Notification",
    tone: "neutral",
    icon: "digest",
    hint: "A message sent to you.",
  },
  question: {
    label: "Question",
    tone: "neutral",
    icon: "help",
    hint: "Domovoi asks you something it cannot find out itself.",
  },
  note: {
    label: "Note",
    tone: "neutral",
    icon: "edit",
    hint: "A comment added by a person or by Domovoi.",
  },
};

export const EDGE_KINDS: Table<EdgeKind> = {
  depends_on: {
    label: "Depends on",
    tone: "neutral",
    icon: "chevron-right",
    hint: "It needed the other step to finish first.",
  },
  caused_by: {
    label: "Caused by",
    tone: "neutral",
    icon: "chevron-right",
    hint: "It exists because of the other step.",
  },
  verifies: {
    label: "Verifies",
    tone: "neutral",
    icon: "chevron-right",
    hint: "It checks the result of the other step.",
  },
  retry_of: {
    label: "Retry of",
    tone: "neutral",
    icon: "chevron-right",
    hint: "It tries again what the other step tried.",
  },
  answers: {
    label: "Answers",
    tone: "neutral",
    icon: "chevron-right",
    hint: "It answers the question the other step asked.",
  },
};
