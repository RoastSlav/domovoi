import { useState, type FormEvent } from "react";
import { useAsk } from "../../api/hooks/incidents";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import { Markdown } from "../../components/Markdown";
import styles from "./incidents.module.css";

/** The longest question the hub accepts. */
export const MAX_QUESTION_CHARS = 1000;

function failureText(error: unknown): string {
  if (error instanceof ApiFailure) {
    if (error.status === 503) {
      return "Triage is off, so there is no one to ask.";
    }
    if (error.status < 500) {
      return error.message;
    }
  }
  return "Could not get an answer. Try again.";
}

/** A follow-up question to triage about this incident, and its answer. */
export function AskBox({ incidentId }: { incidentId: string }) {
  const [question, setQuestion] = useState("");
  const ask = useAsk(incidentId);

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (question.trim()) {
      ask.mutate(question.trim());
    }
  };

  return (
    <form className={styles.ask} onSubmit={onSubmit}>
      <label htmlFor="ask-question">Ask about this incident</label>
      <textarea
        id="ask-question"
        value={question}
        maxLength={MAX_QUESTION_CHARS}
        onChange={(e) => setQuestion(e.target.value)}
        placeholder="For example: is this related to last night's backup?"
      />
      <div>
        <Button type="submit" variant="primary" disabled={ask.isPending || !question.trim()}>
          {ask.isPending ? "Asking…" : "Ask"}
        </Button>
      </div>
      {ask.error && (
        <p className={styles.error} role="alert">
          {failureText(ask.error)}
        </p>
      )}
      {ask.data && (
        <div className={styles.answer} aria-live="polite">
          <Markdown text={ask.data.answer} />
        </div>
      )}
    </form>
  );
}
