import { useEffect, useRef, type ReactNode } from "react";
import { ChatComposer } from "./ChatComposer";
import { InlineMessage } from "./InlineMessage";
import { OperatorTurns, type OperatorLiveText, type OperatorTurn } from "./OperatorTurns";
import styles from "./ChatThread.module.css";

export type { OperatorLiveText, OperatorTurn } from "./OperatorTurns";

/** How an Operator thread is driven: the page owns the turns and the draft. */
export interface OperatorThreadProps {
  /** Why nothing can be asked now; shown in place of the composer hint. */
  disabledReason?: string | undefined;
  /** The draft in the composer. */
  draft: string;
  /** The line under the composer saying what the Operator can and cannot do. */
  hint: ReactNode;
  /** The composer's label. */
  label: string;
  /** Text being written right now for the pending answer. */
  live?: OperatorLiveText | undefined;
  /** Shown beside the reason when there is no model: the way to fix it. */
  offAction?: ReactNode;
  /** With no model to ask, the reason replaces the composer. */
  offReason?: string | undefined;
  onDraftChange: (text: string) => void;
  onRetry?: ((turn: OperatorTurn) => void) | undefined;
  onSend: (question: string) => void;
  /** Stops the pending answer; without it there is no Stop button. */
  onStop?: (() => void) | undefined;
  sending?: boolean | undefined;
  turns: OperatorTurn[];
}

/**
 * A durable conversation with the Operator, read as a transcript above a composer. Unlike
 * `ChatThread`, the turns carry a status and the saved answer replaces the live text when it
 * lands. Use it for the Operator panel and workspace; incident follow-ups keep `ChatThread`.
 */
export function OperatorThread(props: OperatorThreadProps) {
  const { turns, live } = props;
  const end = useRef<HTMLDivElement | null>(null);
  const newest = `${turns.length}:${turns.at(-1)?.status ?? ""}:${live?.text.length ?? 0}`;
  useEffect(() => {
    // jsdom has no layout, so the call may be missing.
    end.current?.scrollIntoView?.({ block: "end" });
  }, [newest]);

  return (
    <div className={styles.chat}>
      <OperatorTurns turns={turns} live={live} onRetry={props.onRetry} onStop={props.onStop} />
      <div ref={end} />
      {props.offReason ? (
        <InlineMessage tone="info" title={props.offReason} action={props.offAction} />
      ) : (
        <ChatComposer
          label={props.label}
          hint={props.hint}
          disabledReason={props.disabledReason}
          sending={props.sending}
          value={props.draft}
          onChange={props.onDraftChange}
          onSend={props.onSend}
        />
      )}
    </div>
  );
}
