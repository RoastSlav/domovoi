import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { isOwnChange } from "../../api/hooks/incidents";
import { useToast } from "../../components";

/** What to say when the incident on screen changes status by itself or from elsewhere. */
const NEWS: Record<string, string> = {
  open: "This incident is open again",
  acked: "This incident was acknowledged",
  muted: "This incident was muted",
  resolved: "This incident resolved",
};

/**
 * Tells the person, with a short toast, when the incident they are looking at changed
 * status without them: it resolved, or someone acknowledged it in chat. Their own changes
 * already report themselves, so those are not announced twice.
 */
export function useStatusNews(id: string, status: string | undefined): void {
  const toast = useToast();
  const client = useQueryClient();
  const before = useRef<string | undefined>(undefined);

  useEffect(() => {
    const was = before.current;
    before.current = status;
    if (!status || !was || was === status || isOwnChange(client, id)) {
      return;
    }
    const title = NEWS[status];
    if (title) {
      toast.info({ title });
    }
  }, [client, id, status, toast]);
}
