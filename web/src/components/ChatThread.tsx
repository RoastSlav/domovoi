import { useEffect, useRef, useState, type ReactNode } from "react";
import { ApiFailure } from "../api/types";
import { describeError } from "../design/messages";
import { Button } from "./Button";
import { ChatComposer } from "./ChatComposer";
import { InlineError } from "./InlineMessage";
import { LoadError } from "./LoadError";
import { Markdown } from "./Markdown";
import { RelativeTime } from "./RelativeTime";
import { Skeleton } from "./Skeleton";
import { Spinner } from "./Spinner";
import { Tag } from "./Tag";
import styles from "./ChatThread.module.css";

export { MAX_QUESTION_CHARS } from "./ChatComposer";

/** The most earlier messages the hub accepts in one request. */
export const MAX_HISTORY_TURNS = 20;

/** A read-only tool the model called while answering. */
export interface ChatTool {
  /** The host it looked at, when it was one host. */
  host?: string;
  name: string;
}

/** One message in a chat: the operator's question or the model's answer. */
export interface ChatTurn {
  /** When it was said; shown beside the text when the conversation is stored elsewhere. */
  at?: string;
  /** Shown under an answer: the actions it proposed, as `ActionCard`s. */
  extra?: ReactNode;
  role: "user" | "assistant";
  text: string;
  /** The tools the model called for an answer, behind a small toggle. */
  tools?: ChatTool[];
}

/** How a chat thread is driven: the page owns the turns, the thread owns the composer. */
export interface ChatThreadProps {
  /** Disables Clear while the conversation is being cleared. */
  clearing?: boolean | undefined;
  /** Why nothing can be asked now ("Triage is off…"); shown under the composer. */
  disabledReason?: string | undefined;
  /** The line under the composer saying what the chat can and cannot do. */
  hint: ReactNode;
  /** The composer's label. */
  label: string;
  /** Why the stored conversation could not be loaded. */
  loadError?: unknown;
  /** Shows that the stored conversation is still loading. */
  loading?: boolean | undefined;
  /** Asks the page to forget the conversation. */
  onClear: () => void;
  /** Loads the stored conversation again after `loadError`. */
  onReload?: (() => void) | undefined;
  /** Sends `question`. A rejection is shown under the question, with Retry. */
  onSend: (question: string) => Promise<unknown>;
  /** Cancels the request in flight; without it there is no Stop button. */
  onStop?: (() => void) | undefined;
  /** The question waiting for its answer, shown with "Thinking…". */
  pending?: string | undefined;
  sending?: boolean | undefined;
  /** Questions offered as buttons while the conversation is empty. */
  starters?: readonly string[] | undefined;
  /** The conversation so far, oldest first. */
  turns: ChatTurn[];
}

/** Errors that will not go away by asking again: the composer is disabled until Retry works. */
const BLOCKING = new Set(["triage_off", "budget_spent"]);

/** Why nothing can be asked, when `error` is one that asking again will not fix. */
function blocks(error: unknown): string | undefined {
  return error instanceof ApiFailure && BLOCKING.has(error.code)
    ? describeError(error).title
    : undefined;
}

/** Whether `error` is a request the person stopped; a `DOMException` is not always an `Error`. */
function isAbort(error: unknown): boolean {
  return (
    typeof error === "object" && error !== null && "name" in error && error.name === "AbortError"
  );
}

function ToolTags({ tools }: { tools: ChatTool[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={styles.tools}>
      <Button size="sm" variant="ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
        {tools.length === 1 ? "1 tool used" : `${tools.length} tools used`}
      </Button>
      {open &&
        tools.map((tool, index) => (
          <Tag key={`${tool.name}-${index}`}>
            {tool.host ? `${tool.name} · ${tool.host}` : tool.name}
          </Tag>
        ))}
    </div>
  );
}

/**
 * A conversation with the model, read as a transcript: the operator's questions on the
 * right on a raised surface, answers on the left as Markdown. The question being answered
 * stays visible with "Thinking…" (and Stop, when the page can cancel); one that failed
 * keeps its text with the reason and Retry under it. While the conversation is empty it
 * offers `starters`.
 *
 * Use it wherever a person talks to the model. The page owns the turns and how they are
 * stored.
 */
export function ChatThread(props: ChatThreadProps) {
  const { turns, pending, sending, onSend, starters } = props;
  const [failed, setFailed] = useState<{ error: unknown; question: string } | null>(null);
  const [draft, setDraft] = useState("");
  const list = useRef<HTMLOListElement | null>(null);

  const newest = `${turns.length}:${turns.at(-1)?.text ?? ""}:${pending ?? ""}:${failed?.question ?? ""}`;
  useEffect(() => {
    // jsdom has no layout, so the call may be missing.
    list.current?.lastElementChild?.scrollIntoView?.({ block: "end" });
  }, [newest]);

  const send = (question: string): void => {
    setFailed(null);
    onSend(question).catch((error: unknown) => {
      if (isAbort(error)) {
        // Stopped on purpose: hand the question back to be edited or sent again.
        setDraft(question);
      } else {
        setFailed({ error, question });
      }
    });
  };

  // With no model to ask there is no conversation to load either: that is said once, under
  // the composer, and not as a failed load.
  const off = blocks(props.loadError);
  const loadError = off ? undefined : props.loadError;
  const blocking = off ?? blocks(failed?.error);
  const empty = turns.length === 0 && !pending && !failed && !props.loading && !props.loadError;

  return (
    <div className={styles.chat}>
      {props.loading && <Skeleton shape="line" count={2} />}
      {Boolean(loadError) && (
        <LoadError
          error={loadError}
          context="load the conversation"
          onRetry={props.onReload ?? (() => undefined)}
        />
      )}
      <ol className={styles.turns} ref={list} aria-label="Conversation">
        {turns.map((turn, index) =>
          turn.role === "user" ? (
            <li className={styles.question} key={`ask-${index}`}>
              <span className="visually-hidden">You asked: </span>
              <span>{turn.text}</span>
              {turn.at && <RelativeTime at={turn.at} />}
            </li>
          ) : (
            <li className={styles.answer} key={`said-${index}`}>
              <span className="visually-hidden">Answer: </span>
              <Markdown text={turn.text} />
              {turn.tools && turn.tools.length > 0 && <ToolTags tools={turn.tools} />}
              {turn.extra}
              {turn.at && <RelativeTime at={turn.at} />}
            </li>
          ),
        )}
        {pending && (
          <>
            <li className={styles.question}>
              <span>{pending}</span>
            </li>
            <li className={styles.pending} role="status">
              <Spinner /> <span>Thinking…</span>
              {props.onStop && (
                <Button size="sm" variant="ghost" onClick={props.onStop}>
                  Stop
                </Button>
              )}
            </li>
          </>
        )}
        {failed && !pending && (
          <>
            <li className={styles.question} data-failed>
              <span>{failed.question}</span>
            </li>
            <li className={styles.failure}>
              <InlineError
                error={failed.error}
                context="get an answer"
                action={
                  <Button size="sm" loading={sending} onClick={() => send(failed.question)}>
                    Retry
                  </Button>
                }
              />
            </li>
          </>
        )}
      </ol>
      {empty && starters && starters.length > 0 && (
        <div className={styles.starters} role="group" aria-label="Suggested questions">
          {starters.map((question) => (
            <Button key={question} size="sm" onClick={() => send(question)}>
              {question}
            </Button>
          ))}
        </div>
      )}
      <ChatComposer
        label={props.label}
        hint={props.hint}
        disabledReason={props.disabledReason ?? blocking}
        sending={sending}
        value={draft}
        onChange={setDraft}
        onSend={send}
      />
      {(turns.length > 0 || failed) && (
        <div>
          <Button
            size="sm"
            variant="ghost"
            icon="trash"
            disabledReason={
              props.clearing || sending ? "Wait for the answer to arrive first." : undefined
            }
            onClick={() => {
              setFailed(null);
              props.onClear();
            }}
          >
            Clear conversation
          </Button>
        </div>
      )}
    </div>
  );
}
