import { ApiFailure, type Issue } from "../api/types";

/**
 * The single place where a failure becomes words. Every hub error code, a network failure,
 * a timeout and anything unknown gets a plain sentence here; components never show a raw
 * code or write their own error text.
 */

/** A failure in words: what to show as the main line, the detail, and what the hub attached. */
export interface DescribedError {
  /** The main line: "Could not approve the action", or the reason when no context is given. */
  title: string;
  /** Why, and what to do: the hub's own message when it is specific, else the catalogue's. */
  detail?: string;
  /** Seconds to wait before trying again, when the hub said so. */
  retryAfter?: number;
  /** For invalid input: every problem with its path. */
  issues?: Issue[];
}

interface Entry {
  /** What happened and what to do, as a sentence. */
  reason: string;
  /** Whether the hub's message for this code says more than the reason. */
  specific: boolean;
}

/** Every error code the hub returns (`crates/domovoi-hub/src/api/error.rs`). */
export const HUB_CODES = [
  "unauthorized",
  "setup_required",
  "bad_credentials",
  "forbidden",
  "not_found",
  "bad_request",
  "invalid",
  "conflict",
  "too_many_requests",
  "unavailable",
  "triage_off",
  "budget_spent",
  "delivery_failed",
  "internal",
] as const;

/** One of the hub's error codes. */
export type HubCode = (typeof HUB_CODES)[number];

const CATALOGUE: Record<HubCode, Entry> = {
  unauthorized: { reason: "Your session ended. Log in again.", specific: false },
  setup_required: {
    reason: "No account exists yet. Open the setup link the hub logged when it started.",
    specific: false,
  },
  bad_credentials: { reason: "Wrong username or password.", specific: false },
  forbidden: { reason: "You are not allowed to do that.", specific: true },
  not_found: { reason: "It no longer exists. It may have been removed.", specific: false },
  bad_request: { reason: "The hub did not accept that.", specific: true },
  invalid: { reason: "Some values are not valid. Fix them and try again.", specific: false },
  conflict: { reason: "That no longer fits the current state.", specific: true },
  too_many_requests: {
    reason: "Too many attempts. Wait a moment and try again.",
    specific: false,
  },
  unavailable: { reason: "A part this needs is not set up or not running.", specific: true },
  triage_off: { reason: "Triage is off, so there is no model to ask.", specific: false },
  budget_spent: {
    reason: "The monthly model budget is spent. It resets next month.",
    specific: false,
  },
  delivery_failed: { reason: "The chat platform did not take the message.", specific: true },
  internal: {
    reason: "The hub ran into a problem. Its log has the details.",
    specific: false,
  },
};

const NETWORK = "The hub did not answer. Check the connection and try again.";
const TIMEOUT = "The hub took too long to answer. Try again.";
const ABORTED = "The request was cancelled.";
const UNKNOWN = "Something went wrong. Try again.";

/** `text` as a sentence: a capital first letter and a full stop. */
function sentence(text: string): string {
  const trimmed = text.trim();
  if (trimmed === "") {
    return trimmed;
  }
  // A message that starts with an identifier ("discord.channel_id must be…") keeps its case.
  const word = /^[a-z]+(?=[\s,:;]|$)/.test(trimmed);
  const capital = word ? trimmed.charAt(0).toUpperCase() + trimmed.slice(1) : trimmed;
  return /[.!?…]$/.test(capital) ? capital : `${capital}.`;
}

/** The sentence to wait out a rate limit of `seconds`. */
export function retryAfterText(seconds: number): string {
  return `Too many attempts. Try again in ${Math.max(0, Math.ceil(seconds))} s.`;
}

function reasonFor(error: unknown): { reason: string; server?: string } {
  if (error instanceof ApiFailure) {
    if (Object.hasOwn(CATALOGUE, error.code)) {
      const entry = CATALOGUE[error.code as HubCode];
      if (error.code === "too_many_requests" && error.retryAfter !== undefined) {
        return { reason: retryAfterText(error.retryAfter) };
      }
      // `invalid` explains itself through its issues; without any, its message is all there is.
      const specific = entry.specific || (error.code === "invalid" && error.issues.length === 0);
      return specific && error.message.trim() !== ""
        ? { reason: entry.reason, server: sentence(error.message) }
        : { reason: entry.reason };
    }
    // Not a hub error body: a proxy answered, or the hub is newer than this UI.
    return { reason: error.status >= 500 ? CATALOGUE.internal.reason : UNKNOWN };
  }
  if (error instanceof DOMException || error instanceof Error) {
    if (error.name === "AbortError") {
      return { reason: ABORTED };
    }
    if (error.name === "TimeoutError") {
      return { reason: TIMEOUT };
    }
    // fetch rejects with a TypeError when the hub cannot be reached.
    if (error instanceof TypeError) {
      return { reason: NETWORK };
    }
  }
  return { reason: UNKNOWN };
}

/**
 * `error` in words. `context` is a short verb phrase ("approve the action"), so the title
 * reads "Could not approve the action" and the detail says why; without it the reason is
 * the title.
 */
export function describeError(error: unknown, context?: string): DescribedError {
  const { reason, server } = reasonFor(error);
  const described: DescribedError = context
    ? { title: `Could not ${context}`, detail: server ?? reason }
    : { title: reason, ...(server ? { detail: server } : {}) };
  if (error instanceof ApiFailure) {
    if (error.retryAfter !== undefined) {
      described.retryAfter = error.retryAfter;
    }
    if (error.issues.length > 0) {
      described.issues = error.issues;
    }
  }
  return described;
}

/** `error` as one line, for places with room for a single sentence. */
export function errorText(error: unknown, context?: string): string {
  const { title, detail } = describeError(error, context);
  const first = /[.!?…]$/.test(title) ? title : `${title}.`;
  return detail ? `${first} ${detail}` : first;
}
