import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ApiFailure } from "../api/types";
import { Button } from "./Button";
import { Markdown } from "./Markdown";
import { RelativeTime } from "./RelativeTime";
import styles from "./ChatThread.module.css";

/** The longest question the hub accepts. */
export const MAX_QUESTION_CHARS = 1000;

/** One message in a chat: the operator's question or the model's answer. */
export interface ChatTurn {
  role: "user" | "assistant";
  text: string;
  /** When it was said; shown before the text when the conversation is stored elsewhere. */
  at?: string;
}

/** What to tell the person when a question failed. */
export function chatFailureText(error: unknown): string {
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

/** How a chat thread is driven: the page owns the turns, the thread owns the composer. */
export interface ChatThreadProps {
  /** The conversation so far, oldest first. */
  turns: ChatTurn[];
  /** The question waiting for its answer, shown with a Thinking bubble. */
  pending?: string | undefined;
  /** Why the last question failed, shown under the composer. */
  error?: string | undefined;
  /** The line under the composer saying what the chat can and cannot do. */
  hint: ReactNode;
  /** The composer's label. */
  label: string;
  /** Disables Send while its answer is on the way. */
  sending?: boolean | undefined;
  /** Disables Clear while the conversation is being cleared. */
  clearing?: boolean | undefined;
  /** Shows that the remembered conversation is still loading. */
  loading?: boolean | undefined;
  /** Shows that the remembered conversation could not be loaded. */
  loadFailed?: boolean | undefined;
  /** Sends `question`; the composer empties once this succeeds, so a failure can be retried. */
  onSend: (question: string) => Promise<unknown>;
  /** Asks the page to forget the conversation. */
  onClear: () => void;
}

/**
 * A conversation with the model: questions right-aligned in accent bubbles and answers
 * left-aligned as Markdown. The next question stays visible with a Thinking bubble while
 * the model works, the composer keeps a question the hub did not answer so it can be
 * retried, and the newest message is scrolled into view.
 */
export function ChatThread({
  turns,
  pending,
  error,
  hint,
  label,
  sending,
  clearing,
  loading,
  loadFailed,
  onSend,
  onClear,
}: ChatThreadProps) {
  const [question, setQuestion] = useState("");
  const composer = useRef<HTMLTextAreaElement | null>(null);
  const turnsList = useRef<HTMLOListElement | null>(null);
  const scrolledTo = useRef("");
  const id = useId();
  const hintId = `${id}-hint`;

  const newest = `${turns.length}:${turns.at(-1)?.text ?? ""}:${pending || ""}`;
  useEffect(() => {
    if (newest !== scrolledTo.current) {
      scrolledTo.current = newest;
      const last = turnsList.current?.lastElementChild;
      if (last) {
        scrollToBottom(last);
      }
    }
  }, [newest]);

  /** Empties the composer after a successful send; React's `value` only seeds a textarea. */
  const clearComposer = (): void => {
    if (composer.current) {
      composer.current.value = "";
    }
    setQuestion("");
  };

  const send = (text: string): void => {
    const trimmed = text.trim();
    if (trimmed && !sending) {
      void onSend(trimmed).then(clearComposer, () => undefined);
    }
  };

  return (
    <div className={styles.chat}>
      {loading && <p className={styles.hint}>Loading…</p>}
      {loadFailed && (
        <p className={styles.error} role="alert">
          Could not load the conversation.
        </p>
      )}
      <ol className={styles.turns} ref={turnsList}>
        {turns.map((turn, index) =>
          turn.role === "user" ? (
            <li className={styles.question} key={`ask-${index}`}>
              {turn.at && <RelativeTime at={turn.at} />} <span>{turn.text}</span>
            </li>
          ) : (
            <li className={styles.answer} key={`said-${index}`}>
              {turn.at && <RelativeTime at={turn.at} />}
              <Markdown text={turn.text} />
            </li>
          ),
        )}
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
      <label htmlFor={id}>{label}</label>
      <textarea
        ref={composer}
        id={id}
        aria-describedby={hintId}
        maxLength={MAX_QUESTION_CHARS}
        value={question}
        onChange={(event) => setQuestion(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            send(question);
          }
        }}
      />
      <p className={styles.hint}>
        {question.length} / {MAX_QUESTION_CHARS}
      </p>
      <p className={styles.hint} id={hintId}>
        {hint}
      </p>
      <div>
        <Button
          variant="primary"
          disabled={sending || !question.trim()}
          onClick={() => send(question)}
        >
          Send
        </Button>
      </div>
      <Button disabled={clearing || sending} onClick={onClear}>
        Clear conversation
      </Button>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
