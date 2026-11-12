import { useEffect, useId, useRef, type ReactNode } from "react";
import { Button } from "./Button";
import { Kbd } from "./Kbd";
import { Textarea } from "./inputs";
import styles from "./ChatThread.module.css";

/** The longest question the hub accepts. */
export const MAX_QUESTION_CHARS = 1000;

/** From this many characters on, the composer shows how many are left. */
const COUNT_FROM = MAX_QUESTION_CHARS - 100;

/** Grows `element` to fit its text; jsdom has no layout, so a zero height is ignored. */
function fit(element: HTMLTextAreaElement | null): void {
  if (!element) {
    return;
  }
  element.style.height = "auto";
  if (element.scrollHeight > 0) {
    element.style.height = `${element.scrollHeight}px`;
  }
}

/**
 * The box a question is written in: one bordered 12 px box with the text and the Send button
 * inside it. It grows with the text, Enter sends and Shift+Enter starts a new line (the line
 * under it says so), and when it cannot be used it says why in place of that line. Part of
 * `ChatThread`; not used on its own.
 */
export function ChatComposer({
  label,
  hint,
  disabledReason,
  sending = false,
  sendVariant = "primary",
  value: question,
  onChange: setQuestion,
  onSend,
}: {
  /** Why nothing can be asked now; shown in place of the hint. */
  disabledReason?: string | undefined;
  hint: ReactNode;
  label: string;
  /** The draft changed; the thread owns it, so a stopped question can be handed back. */
  onChange: (text: string) => void;
  /** Called with the trimmed question; the composer empties at once. */
  onSend: (question: string) => void;
  sending?: boolean | undefined;
  /** `secondary` where the page already has a primary button of its own. */
  sendVariant?: "primary" | "secondary";
  /** The draft. */
  value: string;
}) {
  const box = useRef<HTMLTextAreaElement | null>(null);
  const hintId = useId();
  useEffect(() => fit(box.current), [question]);

  const blocked = Boolean(disabledReason);
  const send = (): void => {
    const trimmed = question.trim();
    if (!trimmed || sending || blocked) {
      return;
    }
    // React's `value` only seeds a textarea once the user has typed into it.
    if (box.current) {
      box.current.value = "";
    }
    setQuestion("");
    onSend(trimmed);
  };

  const why = disabledReason ?? (question.trim() ? undefined : "Write a question first.");
  return (
    <div className={styles.composer}>
      <div className={styles.composerBox}>
        <Textarea
          ref={box}
          className={styles.box}
          aria-label={label}
          aria-describedby={hintId}
          rows={1}
          maxLength={MAX_QUESTION_CHARS}
          value={question}
          readOnly={blocked}
          aria-disabled={blocked || undefined}
          onChange={(event) => setQuestion(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              send();
            }
          }}
        />
        <Button
          variant={sendVariant}
          size="sm"
          loading={sending}
          disabledReason={why}
          onClick={send}
        >
          Send
        </Button>
      </div>
      <div className={styles.composerMeta}>
        <p className={styles.hint} id={hintId}>
          {disabledReason ?? (
            <>
              {hint} <Kbd>Enter</Kbd> sends, <Kbd>Shift</Kbd>+<Kbd>Enter</Kbd> starts a new line.
            </>
          )}
        </p>
        {question.length >= COUNT_FROM && (
          <span className={`${styles.count} tabular-nums`} role="status">
            {question.length} / {MAX_QUESTION_CHARS}
          </span>
        )}
      </div>
    </div>
  );
}
