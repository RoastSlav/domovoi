import { turnStatusMeta } from "../design/tone";
import { Button } from "./Button";
import { InlineMessage } from "./InlineMessage";
import { Markdown } from "./Markdown";
import { RelativeTime } from "./RelativeTime";
import { Spinner } from "./Spinner";
import { StatusPill } from "./StatusPill";
import { ToolTags, type ChatTool } from "./ChatToolTags";
import styles from "./ChatThread.module.css";

/** One message of an Operator conversation, as the hub stored it. */
export interface OperatorTurn {
  /** Only the last assistant message, and only when it failed or was stopped. */
  canRetry?: boolean;
  /** When it was said (ISO 8601). */
  at: string;
  id: string;
  /** Words for a failed or stopped answer, from `describeTurnFailure`. */
  problem?: { detail?: string; title: string };
  role: "user" | "assistant";
  status: "complete" | "pending" | "failed" | "cancelled";
  text: string;
  tools?: ChatTool[];
}

/** What the model is writing right now, before it is saved. */
export interface OperatorLiveText {
  /** The safe text so far; replaced by the saved answer once it lands. */
  text: string;
  /** Tools it is looking at now. */
  tools: ChatTool[];
}

/**
 * The transcript of an Operator conversation: questions on the right, saved answers as
 * Markdown, the answer being written as live text with Stop, and a failed or stopped answer
 * with its reason and Retry. Part of `OperatorThread`.
 */
export function OperatorTurns({
  turns,
  live,
  onRetry,
  onStop,
}: {
  live?: OperatorLiveText | undefined;
  onRetry?: ((turn: OperatorTurn) => void) | undefined;
  onStop?: (() => void) | undefined;
  turns: OperatorTurn[];
}) {
  return (
    <ol className={styles.turns} aria-label="Conversation">
      {turns.map((turn) => {
        if (turn.role === "user") {
          return (
            <li className={styles.question} key={turn.id}>
              <span className="visually-hidden">You asked: </span>
              <span>{turn.text}</span>
              <RelativeTime at={turn.at} />
            </li>
          );
        }
        if (turn.status === "pending") {
          return (
            <li className={styles.answer} key={turn.id}>
              <div role="status" aria-label="Answering">
                {live && live.text !== "" ? <Markdown text={live.text} /> : <Spinner />}
              </div>
              <span className={styles.pending}>
                <StatusPill meta={turnStatusMeta("pending")} size="sm" />
                {onStop && (
                  <Button size="sm" variant="ghost" onClick={onStop}>
                    Stop
                  </Button>
                )}
              </span>
              {live && live.tools.length > 0 && <ToolTags tools={live.tools} />}
            </li>
          );
        }
        if (turn.status === "failed" || turn.status === "cancelled") {
          return (
            <li className={styles.failure} key={turn.id}>
              <InlineMessage
                tone={turn.status === "failed" ? "warning" : "info"}
                title={turn.problem?.title ?? turnStatusMeta(turn.status).label}
                action={
                  turn.canRetry && onRetry ? (
                    <Button size="sm" onClick={() => onRetry(turn)}>
                      Retry
                    </Button>
                  ) : undefined
                }
              >
                {turn.problem?.detail}
              </InlineMessage>
            </li>
          );
        }
        return (
          <li className={styles.answer} key={turn.id}>
            <span className="visually-hidden">Answer: </span>
            <Markdown text={turn.text} />
            {turn.tools && turn.tools.length > 0 && <ToolTags tools={turn.tools} />}
            <RelativeTime at={turn.at} />
          </li>
        );
      })}
    </ol>
  );
}
