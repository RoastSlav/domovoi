import { useEffect, useRef, useState } from "react";
import { useAsk, useClearFollowUps, useFollowUps } from "../../api/hooks/incidents";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Markdown } from "../../components/Markdown";
import { RelativeTime } from "../../components/RelativeTime";
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

/** Brings the newest message into view; jsdom has no layout, so the call may be missing. */
function scrollToBottom(element: Element): void {
  element.scrollIntoView?.({ block: "end" });
}

/**
 * The chat with triage about one incident: the turns the model still remembers,
 * questions right-aligned in accent bubbles and answers left-aligned as Markdown; the
 * next question stays visible with a Thinking bubble while the model works, and the
 * Clear conversation button has it forget the whole thing.
 */
export function IncidentChat({ incidentId, host }: { incidentId: string; host: string }) {
  const remembered = useFollowUps(incidentId);
  const ask = useAsk(incidentId);
  const clear = useClearFollowUps(incidentId);
  const [question, setQuestion] = useState("");
  const [sending, setSending] = useState("");
  const [clearOpen, setClearOpen] = useState(false);
  const turnsList = useRef<HTMLOListElement | null>(null);
  const scrolledTo = useRef("");

  const turns = remembered.data?.turns ?? [];
  const asked = sending.trim();
  const landed = ask.isSuccess && turns.some((turn) => turn.question === asked);
  const pending = asked && !ask.error && !landed ? asked : "";

  const send = (text: string): void => {
    const trimmed = text.trim();
    if (trimmed && !ask.isPending) {
      setSending(trimmed);
      ask.mutate(trimmed);
    }
  };

  const newestTurn = `${turns.length}:${turns.at(-1)?.question ?? ""}:${pending}`;
  useEffect(() => {
    if (newestTurn !== scrolledTo.current) {
      scrolledTo.current = newestTurn;
      const newest = turnsList.current?.lastElementChild;
      if (newest) {
        scrollToBottom(newest);
      }
    }
  }, [newestTurn]);

  return (
    <div className={styles.chat}>
      <ol className={styles.turns} ref={turnsList}>
        {turns.flatMap((turn, index) => [
          <li className={styles.question} key={`ask-${index}`}>
            <RelativeTime at={turn.at} /> <span>{turn.question}</span>
          </li>,
          <li className={styles.answer} key={`said-${index}`}>
            <RelativeTime at={turn.at} />
            <Markdown text={turn.answer} />
          </li>,
        ])}
        {pending && (
          <>
            <li className={styles.question}>
              <span>{pending}</span>
            </li>
            <li className={styles.pending} aria-live="polite">
              Thinking…
            </li>
          </>
        )}
      </ol>
      <label htmlFor="chat-input">Ask about this incident</label>
      <textarea
        id="chat-input"
        aria-describedby="chat-hint"
        value={question}
        onChange={(event) => setQuestion(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            send(question);
          }
        }}
      />
      <p className={styles.hint}>
        {question.length} / {MAX_QUESTION_CHARS}
      </p>
      <p className={styles.hint} id="chat-hint">
        Uses read-only tools on {host}. It cannot change anything.
      </p>
      <div>
        <Button
          variant="primary"
          disabled={ask.isPending || clear.isPending || !question.trim()}
          onClick={() => send(question)}
        >
          Send
        </Button>
      </div>
      <Button disabled={clearOpen || clear.isPending} onClick={() => setClearOpen(true)}>
        Clear conversation
      </Button>
      {ask.error && (
        <p className={styles.error} role="alert">
          {failureText(ask.error)}
        </p>
      )}
      {clearOpen && (
        <ConfirmDialog
          title="Clear the conversation?"
          confirmLabel="Yes, clear it"
          busy={clear.isPending}
          onConfirm={() => clear.mutate(undefined, { onSuccess: () => setClearOpen(false) })}
          onCancel={() => setClearOpen(false)}
        >
          <p>The model forgets this conversation. The questions stay in the timeline.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
