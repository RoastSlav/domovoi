import { useState } from "react";
import { useAsk, useClearFollowUps, useFollowUps } from "../../api/hooks/incidents";
import { ChatThread, type ChatTurn, ConfirmDialog, useActionFeedback } from "../../components";
import { useChatOff } from "../chat/useChatOff";

export { MAX_QUESTION_CHARS } from "../../components";

/**
 * The chat with triage about one incident: the turns the model still remembers, the next
 * question visible with "Thinking…" while the model works, an answer the hub did not manage
 * to remember shown from the response, and Clear conversation to have it forget the lot.
 */
export function IncidentChat({ incidentId, host }: { incidentId: string; host: string }) {
  const remembered = useFollowUps(incidentId);
  const ask = useAsk(incidentId);
  const off = useChatOff();
  const clear = useActionFeedback(useClearFollowUps(incidentId), {
    success: "Conversation cleared",
    error: "clear the conversation",
    errorDisplay: "inline",
  });
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
    thread.push({ role: "assistant", text: turn.answer });
  }
  if (unremembered) {
    thread.push({ role: "user", text: asked }, { role: "assistant", text: unremembered });
  }

  return (
    <>
      <ChatThread
        turns={thread}
        pending={pending}
        hint={`Uses read-only tools on ${host}. It cannot change anything.`}
        label="Ask about this incident"
        disabledReason={off}
        sending={ask.isPending}
        clearing={clearOpen || clear.isPending}
        loading={remembered.isPending}
        loadError={remembered.data ? undefined : remembered.error}
        onReload={() => void remembered.refetch()}
        onSend={(question) => {
          setSending(question);
          return ask.mutateAsync(question);
        }}
        onClear={() => setClearOpen(true)}
      />
      {clearOpen && (
        <ConfirmDialog
          title="Clear the conversation?"
          consequence="The model forgets this conversation. The questions stay in the timeline."
          confirmLabel="Clear conversation"
          busy={clear.isPending || ask.isPending}
          error={clear.error}
          onConfirm={() =>
            clear.mutate(undefined, {
              onSuccess: () => {
                setClearOpen(false);
                setSending("");
              },
            })
          }
          onCancel={() => {
            setClearOpen(false);
            clear.reset();
          }}
        />
      )}
    </>
  );
}
