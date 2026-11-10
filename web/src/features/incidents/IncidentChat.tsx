import { useState } from "react";
import { useAsk, useClearFollowUps, useFollowUps } from "../../api/hooks/incidents";
import { ChatThread, chatFailureText, type ChatTurn } from "../../components/ChatThread";
import { ConfirmDialog } from "../../components/ConfirmDialog";

export { MAX_QUESTION_CHARS } from "../../components/ChatThread";

/**
 * The chat with triage about one incident: the turns the model still remembers,
 * questions right-aligned in accent bubbles and answers left-aligned as Markdown; the
 * next question stays visible with a Thinking bubble while the model works, an answer
 * the hub did not manage to remember is shown from the response, and the Clear
 * conversation button has it forget the whole thing.
 */
export function IncidentChat({ incidentId, host }: { incidentId: string; host: string }) {
  const remembered = useFollowUps(incidentId);
  const ask = useAsk(incidentId);
  const clear = useClearFollowUps(incidentId);
  const [sending, setSending] = useState("");
  const [clearOpen, setClearOpen] = useState(false);

  const turns = remembered.data?.turns ?? [];
  const asked = sending.trim();
  const landed = ask.isSuccess && turns.some((turn) => turn.question === asked);
  // The hub can answer successfully without recording the turn, when storing it
  // failed; show that answer from the response so it never gets lost.
  const unremembered = ask.isSuccess && asked && !landed ? (ask.data?.answer ?? "") : "";
  const pending = asked && !ask.error && !ask.isSuccess ? asked : "";

  const thread: ChatTurn[] = [];
  for (const turn of turns) {
    thread.push({ role: "user", text: turn.question, at: turn.at });
    thread.push({ role: "assistant", text: turn.answer, at: turn.at });
  }
  if (unremembered) {
    thread.push({ role: "user", text: asked }, { role: "assistant", text: unremembered });
  }

  return (
    <>
      <ChatThread
        turns={thread}
        pending={pending}
        error={ask.error ? chatFailureText(ask.error) : undefined}
        hint={`Uses read-only tools on ${host}. It cannot change anything.`}
        label="Ask about this incident"
        sending={ask.isPending}
        clearing={clearOpen || clear.isPending}
        loading={remembered.isPending}
        loadFailed={Boolean(remembered.error)}
        onSend={(question) => {
          setSending(question);
          return ask.mutateAsync(question).then(() => undefined);
        }}
        onClear={() => setClearOpen(true)}
      />
      {clearOpen && (
        <ConfirmDialog
          title="Clear the conversation?"
          confirmLabel="Yes, clear it"
          busy={clear.isPending || ask.isPending}
          onConfirm={() =>
            clear.mutate(undefined, {
              onSuccess: () => {
                setClearOpen(false);
                setSending("");
              },
            })
          }
          onCancel={() => setClearOpen(false)}
        >
          <p>The model forgets this conversation. The questions stay in the timeline.</p>
        </ConfirmDialog>
      )}
    </>
  );
}
